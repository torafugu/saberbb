#!/usr/bin/env python3
"""Frozen-contact EV x VLA / first-ground and tangential-impulse sensitivity."""
import argparse
import json
import math
import sqlite3
from collections import Counter, defaultdict
from pathlib import Path

import bat_vibration_profile as profile
import vibration_profile_probe as baseline
from contact_3d import ContactGeometry, band, vector
from contact_collision_probe import write_csv
from contact_spin import BALL_RADIUS_M, first_ground, solve_side
from effective_mass_probe import direction
from forecast_contact_geometry import vec
from observation_contact_source import ContactSource

SPIN_MODELS = (('nathan_full_mu0.2_et0', .2, 0.), ('nathan_full_mu0.2_et0.4', .2, .4))
MODELS = baseline.MODELS + tuple(name for name,_,_ in SPIN_MODELS)
KEYS = ['scope','variant','model','height_m','stage','direction','band_1','band_2']
GROUND_METRICS = ['first_ground_time_sec','first_ground_distance_m','pre_ground_horizontal_ms',
    'pre_ground_speed_ms','post_ground_speed_ms','post_ground_horizontal_ms','post_ground_spin_rpm',
    'rebound_height_m','second_ground_time_sec','rebound_travel_m','ground_loss_j']
COLLISION_METRICS = ['ev_ms','vla_deg','spin_rpm','ball_translation_before_j','ball_translation_after_j',
    'ball_rotation_before_j','ball_rotation_after_j','system_before_j','system_after_j','normal_loss_j',
    'tangent_loss_j','total_loss_j','normal_impulse_kg_ms','tangent_impulse_kg_ms']
METRICS = COLLISION_METRICS + GROUND_METRICS
PAIR_KEYS = ['scope','variant','height_m','before_model','after_model']


def event(r, checks, ball_spin):
    results, eligibility = baseline.event(r, checks)
    for m in results.values():
        if m['collision_status'] == 'impulse_contact':
            m['outgoing_spin_rad_s'] = ball_spin
            m['spin_rpm'] = math.sqrt(sum(w*w for w in ball_spin))*30/math.pi
    for name, mu, et in SPIN_MODELS:
        if eligibility != 'eligible':
            results[name] = dict(collision_status=eligibility, ev_ms=None)
            continue
        g = ContactGeometry(r['entry_since_release_sec'], vec(r,'axis_at_entry_'), vec(r,'normal_'),
            r['axial_at_entry_m'], r['closest_axial_at_entry_m'], r['half_length_m'], 'side')
        results[name] = m = solve_side(g, vec(r,'ball_velocity_'), vec(r,'point_velocity_'), ball_spin,
            tuple(float(r['omega_'+a+'_rad_s']) for a in 'xyz'), profile.MASS_KG,
            profile.MASS_KG*profile.GYRATION_M**2, profile.CM_Z_M-profile.REFERENCE_Z_M,
            profile.profile_e(r['_source_z_m']), mu, et)
        check_metrics(checks, 'spin_', m.get('checks', {}))
    return results, eligibility


def check_metrics(checks, prefix, values):
    for key, value in values.items():
        if not math.isfinite(value) or value > 1e-7:
            raise ValueError(prefix+'identity_mismatch:'+key)
        checks[prefix+key] = max(checks[prefix+key], value)


class Aggregate:
    def __init__(self, sample_limit):
        self.groups = defaultdict(Counter)
        self.statuses = Counter()
        self.pairs = defaultdict(Counter)
        self.samples = []
        self.sample_limit = sample_limit

    def add(self, r, results, heights, ground_e, ground_mu, slow_ms, checks):
        scopes = ['all_swings'] + (['recorded_batting_only'] if r['adjusted_contact_type'] != 'UnrecordedBatting' else [])
        evaluated = {}
        for name, m in results.items():
            self.statuses[r['variant'],name,m['collision_status']] += 1
            for height in heights:
                ground = None
                if m['collision_status'] == 'impulse_contact':
                    ground = first_ground(m['outgoing_world_ms'],m['outgoing_spin_rad_s'],height,ground_e,ground_mu)
                    check_metrics(checks,'', {k:v for k,v in ground.items() if k in (
                        'ground_energy_balance_j','ground_negative_dissipation_j','flight_energy_balance_j')})
                    evaluated[name,height] = ground
                    slow = ground['post_ground_horizontal_ms'] < slow_ms
                    downward = m.get('vla_deg') is not None and m['vla_deg'] < 0
                    if len(self.samples) < self.sample_limit and m['ev_ms'] >= 40 and downward and slow:
                        sample = dict(game_id=r['game_id'],count_seq=r['count_seq'],variant=r['variant'],model=name,
                            height_m=height,hla_deg=m['hla_deg'],**{k:m.get(k) for k in COLLISION_METRICS},**ground)
                        self.samples.append(sample)
                for scope in scopes:
                    strata = [('overall','all','all','all')]
                    if ground is not None:
                        strata.append(('ev_vla',direction(m),band(m['ev_ms'],(25,40)),band(m['vla_deg'],(-60,-30,-10,0,10,30,60))))
                        strata.append(('ground_response',direction(m),band(ground['post_ground_horizontal_ms'],(10,25,40)),band(ground['rebound_height_m'],(.5,2,5,10))))
                    else:
                        strata.append(('ev_vla',direction(m),'not_evaluated','not_evaluated'))
                        strata.append(('ground_response',direction(m),'not_evaluated','not_evaluated'))
                    for stage,d,eb,vb in strata:
                        g = self.groups[scope,r['variant'],name,height,stage,d,eb,vb]
                        g['states'] += 1
                        if ground is None:continue
                        g['evaluated_contacts'] += 1
                        g['fair_contacts'] += direction(m) == 'fair_direction'
                        fair = direction(m) == 'fair_direction'
                        g['high_ev_downward_contacts'] += m['ev_ms'] >= 40 and downward
                        g['low_ev_contacts'] += m['ev_ms'] < 25
                        g['slow_after_ground_contacts'] += slow
                        g['high_ev_downward_slow_contacts'] += m['ev_ms'] >= 40 and downward and slow
                        g['fair_low_ev_contacts'] += fair and m['ev_ms'] < 25
                        g['fair_slow_after_ground_contacts'] += fair and slow
                        g['fair_high_ev_downward_slow_contacts'] += fair and m['ev_ms'] >= 40 and downward and slow
                        for metric in METRICS:
                            value = ground.get(metric,m.get(metric))
                            if value is not None:
                                g[metric] += value
                                g['n_'+metric] += 1
        for after,_,_ in SPIN_MODELS:
            before = baseline.FULL
            for height in heights:
                left,right = evaluated.get((before,height)),evaluated.get((after,height))
                for scope in scopes:
                    p = self.pairs[scope,r['variant'],height,before,after]
                    p['states'] += 1
                    if left is None or right is None:continue
                    p['paired_contacts'] += 1
                    fair = direction(results[before]) == direction(results[after]) == 'fair_direction'
                    p['paired_fair_contacts'] += fair
                    p['before_fair_after_nonfair'] += direction(results[before]) == 'fair_direction' and direction(results[after]) != 'fair_direction'
                    p['before_nonfair_after_fair'] += direction(results[before]) != 'fair_direction' and direction(results[after]) == 'fair_direction'
                    # Keep a common paired denominator; an angle change to foul is visible separately.
                    ls,rs = left['post_ground_horizontal_ms'] < slow_ms,right['post_ground_horizontal_ms'] < slow_ms
                    p['slow_to_slow' if ls and rs else 'slow_to_fast' if ls else 'fast_to_slow' if rs else 'fast_to_fast'] += 1
                    for label,m,g in (('before',results[before],left),('after',results[after],right)):
                        for k in ('ev_ms','vla_deg','spin_rpm','post_ground_horizontal_ms','rebound_height_m'):
                            value = g.get(k,m.get(k))
                            if value is not None:
                                p[label+'_'+k] += value
                                p['n_'+label+'_'+k] += 1

    def write(self, output):
        counts = ['states','evaluated_contacts','fair_contacts','high_ev_downward_contacts','low_ev_contacts',
            'slow_after_ground_contacts','high_ev_downward_slow_contacts','fair_low_ev_contacts',
            'fair_slow_after_ground_contacts','fair_high_ev_downward_slow_contacts']
        rows = []
        for key,g in sorted(self.groups.items()):
            row = dict(zip(KEYS,key),**{k:g[k] for k in counts})
            row['unavailable_states'] = g['states']-g['evaluated_contacts']
            for k in METRICS:
                row['n_'+k] = g['n_'+k]
                row['avg_'+k] = g[k]/g['n_'+k] if g['n_'+k] else None
            rows.append(row)
        write_csv(output/'ground_spin_summary.csv',KEYS+counts+['unavailable_states']+[p+k for k in METRICS for p in ('n_','avg_')],rows)
        keys = ['variant','model','collision_status']
        write_csv(output/'ground_spin_status.csv',keys+['states'],[dict(zip(keys,k),states=n) for k,n in sorted(self.statuses.items())])
        counts = ['states','paired_contacts','paired_fair_contacts','before_fair_after_nonfair','before_nonfair_after_fair','slow_to_slow','slow_to_fast','fast_to_slow','fast_to_fast']
        metrics = [label+'_'+k for label in ('before','after') for k in ('ev_ms','vla_deg','spin_rpm','post_ground_horizontal_ms','rebound_height_m')]
        rows = []
        for key,p in sorted(self.pairs.items()):
            row = dict(zip(PAIR_KEYS,key),**{k:p[k] for k in counts})
            for k in metrics:
                row['n_'+k] = p['n_'+k]
                row['avg_'+k] = p[k]/p['n_'+k] if p['n_'+k] else None
            rows.append(row)
        write_csv(output/'ground_spin_pairs.csv',PAIR_KEYS+counts+[prefix+k for k in metrics for prefix in ('n_','avg_')],rows)
        fields = ['game_id','count_seq','variant','model','height_m','hla_deg']+COLLISION_METRICS+GROUND_METRICS+[
            'ground_energy_balance_j','ground_negative_dissipation_j','flight_energy_balance_j']
        write_csv(output/'ground_spin_examples.csv',fields,self.samples)


def run(input_dir, output, heights=(.6,.9,1.2), ball_spin=(0.,0.,0.), ground_e=.45, ground_mu=.25, slow_ms=25., sample_limit=500):
    from contact_spin import parameters
    parameters(ground_e,ground_mu,0.)
    if (not heights or len(set(heights)) != len(heights) or any(not math.isfinite(h) or h <= 0 for h in heights)
        or not vector(ball_spin) or not math.isfinite(slow_ms) or slow_ms <= 0
        or not isinstance(sample_limit,int) or sample_limit < 0):
        raise ValueError('invalid_probe_parameters')
    source = ContactSource(input_dir)
    output = Path(output)
    output.mkdir(parents=True,exist_ok=False)
    checks = defaultdict(float);counts = Counter();aggregate = Aggregate(sample_limit)
    for r in source.rows(checks):
        results,eligibility = event(r,checks,ball_spin)
        aggregate.add(r,results,heights,ground_e,ground_mu,slow_ms,checks)
        counts['trial_rows'] += 1
        counts['source_geometry_contacts'] += r['status'] == 'contact'
        counts['source_impulse_contacts'] += r['collision_status'] == 'impulse_contact'
        counts[eligibility] += 1
    aggregate.write(output)
    coverage = {k:counts[k] for k in ('trial_rows','source_geometry_contacts','source_impulse_contacts',
        'eligible','not_evaluated:unsupported_cap','not_evaluated:out_of_curve','not_evaluated:non_approaching',
        'not_evaluated:no_contact','not_evaluated:overlap_at_start','not_evaluated:unresolved')}
    coverage.update(models=len(MODELS),height_scenarios=len(heights))
    write_csv(output/'ground_spin_coverage.csv',list(coverage),[coverage])
    write_csv(output/'ground_spin_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    metadata = dict(version=1,input_sha256=source.hashes,input_metadata=source.meta,models=MODELS,
        bat=profile.metadata(),spin_models=[dict(name=n,mu=mu,et=et) for n,mu,et in SPIN_MODELS],
        heights_m=heights,incoming_ball_spin_world_rad_s=ball_spin,ball_radius_m=BALL_RADIUS_M,
        ground_restitution=ground_e,ground_friction=ground_mu,slow_horizontal_threshold_ms=slow_ms,
        assumptions=[
            'All height and incoming ball-spin values are explicit scenarios, absent from source CSV.',
            'Normal-only baselines carry the same incident spin unchanged; tangential models solve spin and velocity together.',
            'Centerline bat point impulse; no finite barrel radius, axial bat torsion or normal-tangent cross-coupling.',
            'mu=0.2 and et=0/0.4 are sensitivity assumptions, not fitted or published bat coefficients.',
            'Gravity-only analytic flight, first ground impact, rebound apex and next-ground travel. No drag, Magnus, fences, fielders or rolling.',
            'Height is center travel to first ground contact; distance origin is the launch point.',
            'Slow after ground is a horizontal-speed diagnostic, not an out/hit result or a calibrated weak-contact label.',
            'Every launch eventually reaches ground in this unbounded model; elevated trajectories need full-flight validation.',
            'Source conditions, scopes, models, heights and strata overlap: never sum them as independent trials.',
            'Example CSV is the first matching high-EV downward slow contacts up to sample_limit, not a representative sample.'],
        sample_limit=sample_limit,source_url='https://baseball.physics.illinois.edu/ObliqueCollisionsWSU-AJP.pdf')
    (output/'ground_spin_metadata.json').write_text(json.dumps(metadata,indent=2,allow_nan=False)+'\n')
    return coverage


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,required=True);p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--heights-m',type=float,nargs='+',default=(.6,.9,1.2))
    p.add_argument('--ball-spin-rad-s',type=float,nargs=3,default=(0.,0.,0.),metavar=('X','Y','Z'))
    p.add_argument('--ground-e',type=float,default=.45);p.add_argument('--ground-mu',type=float,default=.25)
    p.add_argument('--slow-ms',type=float,default=25.);p.add_argument('--sample-limit',type=int,default=500)
    a = p.parse_args()
    try:
        print(run(a.input_dir,a.output_dir,tuple(a.heights_m),tuple(a.ball_spin_rad_s),a.ground_e,a.ground_mu,a.slow_ms,a.sample_limit))
    except (OSError,ValueError,sqlite3.Error) as exc:
        p.exit(1,f'Diagnostic failed: {exc}\n')
