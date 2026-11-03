#!/usr/bin/env python3
"""Frozen-contact attribution of local speed, rigid recoil and positional e_eff.

Full 2^3 factorial diagnostic; no additional hand/body loss term. Counterfactuals
normalize parameters at saved s=0 while retaining each contact's normal/axis.
"""
import argparse
import json
import math
import sqlite3
from collections import Counter, defaultdict
from pathlib import Path

import bat_vibration_profile as profile
import vibration_profile_probe as baseline
from contact_3d import ContactGeometry, band
from contact_collision_probe import BALL_MASS, cross, dot, write_csv
from contact_recoil import inverse_mass, solve, recoil_checks
from effective_mass_probe import direction
from forecast_contact_geometry import vec
from observation_contact_source import ContactSource
from rotating_bat_probe import add, sub, mul, norm

FACTORS = ('local_velocity', 'rigid_recoil', 'positional_e')
KEYS = ['scope', 'variant', 'stage', 'band_1', 'band_2']
OUTCOMES = ('ev_ms', 'ball_energy_j')
DESCRIPTORS = ('saved_axial_m', 'source_z_m', 'distance_from_cm_m', 'point_speed_ms',
    'reference_point_speed_ms', 'point_normal_ms', 'reference_point_normal_ms',
    'approach_speed_ms', 'reference_approach_speed_ms', 'effective_mass_kg',
    'reference_effective_mass_kg', 'applied_e', 'reference_ev_ms', 'actual_ev_ms',
    'actual_vla_deg', 'ball_before_j', 'ball_after_j',
    'bat_translation_before_j', 'bat_translation_after_j', 'bat_rotation_before_j', 'bat_rotation_after_j',
    'bat_before_j', 'bat_after_j',
    'bat_energy_change_j', 'effective_loss_j')


def check(checks, name, error):
    if not math.isfinite(error) or error > 1e-7:
        raise ValueError('axial_identity_mismatch:' + name)
    checks[name] = max(checks[name], error)


def shapley(values):
    """Signed contributions, averaged over all factor orders. Keys are bit masks."""
    if set(values) != set(range(8)) or not all(math.isfinite(v) for v in values.values()):
        raise ValueError('invalid_factorial_values')
    result = {}
    for i, name in enumerate(FACTORS):
        bit = 1 << i
        result[name] = sum((1/3 if mask.bit_count() in (0, 2) else 1/6) *
            (values[mask | bit] - values[mask]) for mask in range(8) if not mask & bit)
    return result


def energy_budget(m, vin, vbat, omega, normal, arm, inertia):
    """Lab-frame rigid energy budget. Loss is lumped, NOT measured vibration/body energy."""
    j = m['normal_impulse_kg_ms']; impulse = mul(normal, j)
    center = sub(vbat, cross(omega, arm))
    after_center = sub(center, mul(impulse, 1/profile.MASS_KG))
    after_omega = sub(omega, mul(cross(arm, impulse), 1/inertia))
    ball_before = .5*BALL_MASS*dot(vin, vin)
    ball_after = .5*BALL_MASS*dot(m['outgoing_world_ms'], m['outgoing_world_ms'])
    translation_before = .5*profile.MASS_KG*dot(center, center)
    translation_after = .5*profile.MASS_KG*dot(after_center, after_center)
    rotation_before = .5*inertia*dot(omega, omega)
    rotation_after = .5*inertia*dot(after_omega, after_omega)
    bat_before = translation_before + rotation_before
    bat_after = translation_after + rotation_after
    loss = .5*j*(1-m['applied_e'])*(-m['relative_normal_ms'])
    return dict(ball_before_j=ball_before, ball_after_j=ball_after,
        bat_translation_before_j=translation_before, bat_translation_after_j=translation_after,
        bat_rotation_before_j=rotation_before, bat_rotation_after_j=rotation_after, bat_before_j=bat_before,
        bat_after_j=bat_after, bat_energy_change_j=bat_after-bat_before, effective_loss_j=loss)


def event(r, checks):
    old, eligibility = baseline.event(r, checks)
    if eligibility != 'eligible':
        return dict(status=eligibility, factorial=None, contributions=None, descriptors=None)
    axis, normal = vec(r, 'axis_at_entry_'), vec(r, 'normal_')
    vin, vbat = vec(r, 'ball_velocity_'), vec(r, 'point_velocity_')
    omega = tuple(float(r['omega_'+a+'_rad_s']) for a in 'xyz')
    s = r['closest_axial_at_entry_m']
    # Saved centerline kinematics: v(s)=v(0)+omega x (axis*s).
    reference_vbat = sub(vbat, cross(omega, mul(axis, s)))
    check(checks, 'reference_velocity_reconstruction_ms', norm(sub(vbat, add(reference_vbat, cross(omega, mul(axis, s))))))
    g = ContactGeometry(r['entry_since_release_sec'], axis, normal, r['axial_at_entry_m'], s, r['half_length_m'], 'side')
    e = profile.profile_e(r['_source_z_m'])
    recoil = [inverse_mass(axis, normal, axial, r['lever_m'], 'free', profile.MASS_KG,
        profile.GYRATION_M, profile.CM_Z_M-profile.REFERENCE_Z_M) for axial in (0., s)]
    factorial = {}
    for mask in range(8):
        velocity = vbat if mask & 1 else reference_vbat
        inv, arm, inertia = recoil[bool(mask & 2)]
        restitution = e if mask & 4 else .5
        m = solve(g, vin, velocity, inv, restitution)
        m['applied_e'] = restitution
        for k, error in recoil_checks(m, vin, velocity, omega, normal, arm, inertia, 'free', profile.MASS_KG, restitution).items():
            check(checks, 'factorial_'+k, error)
        if m['collision_status'] == 'impulse_contact':
            m['ball_energy_j'] = .5*BALL_MASS*dot(m['outgoing_world_ms'], m['outgoing_world_ms'])
        else:
            m['ball_energy_j'] = None
        factorial[mask] = m
    actual = factorial[7]
    check(checks, 'actual_profile_outgoing_ms', norm(sub(actual['outgoing_world_ms'], old[baseline.FULL]['outgoing_world_ms'])))
    budget = energy_budget(actual, vin, vbat, omega, normal, recoil[1][1], recoil[1][2])
    check(checks, 'lab_energy_balance_j', abs(budget['ball_after_j']+budget['bat_after_j']-
        budget['ball_before_j']-budget['bat_before_j']+budget['effective_loss_j']))
    complete = all(m['collision_status'] == 'impulse_contact' for m in factorial.values())
    contributions = None
    if complete:
        contributions = {outcome:shapley({mask:m[outcome] for mask,m in factorial.items()}) for outcome in OUTCOMES}
        for outcome in OUTCOMES:
            check(checks, 'attribution_closure_'+outcome, abs(sum(contributions[outcome].values())-
                (factorial[7][outcome]-factorial[0][outcome])))
    descriptors = dict(saved_axial_m=s, source_z_m=r['_source_z_m'],
        distance_from_cm_m=s-(profile.CM_Z_M-profile.REFERENCE_Z_M), point_speed_ms=norm(vbat),
        reference_point_speed_ms=norm(reference_vbat), point_normal_ms=dot(vbat, normal),
        reference_point_normal_ms=dot(reference_vbat, normal), approach_speed_ms=-actual['relative_normal_ms'],
        reference_approach_speed_ms=max(0.,-dot(sub(vin, reference_vbat), normal)),
        effective_mass_kg=actual['effective_mass_kg'], reference_effective_mass_kg=1/recoil[0][0],
        applied_e=e, reference_ev_ms=factorial[0]['ev_ms'], actual_ev_ms=actual['ev_ms'],
        actual_vla_deg=actual['vla_deg'], **budget)
    return dict(status='eligible' if complete else 'eligible:counterfactual_nonapproaching',
        factorial=factorial, contributions=contributions, descriptors=descriptors)


def strata(r, actual_direction):
    yield 'overall', 'all', 'all'
    yield 'source_z', band(r['_source_z_m'], (.60,.64,.68,.72,.76,.80,.84)), r['contact_region'] or 'no_entry'
    yield 'source_z_direction', band(r['_source_z_m'], (.60,.64,.68,.72,.76,.80,.84)), actual_direction
    s = r['closest_axial_at_entry_m'] if r['status'] == 'contact' else None
    yield 'saved_axial', band(s, (-.12,-.06,0,.06,.12)), r['contact_region'] or 'no_entry'
    yield 'normal_approach', band(r['normal_speed_fraction'], (.5,.8,.95)), 'all'


class Aggregate:
    def __init__(self, sample_limit):
        self.summary = defaultdict(Counter)
        self.factorial = defaultdict(Counter)
        self.attribution = defaultdict(Counter)
        self.status = Counter()
        self.examples = []; self.sample_limit = sample_limit

    def add(self, r, event):
        self.status[r['variant'], event['status']] += 1
        scopes = ['all_swings'] + (['recorded_batting_only'] if r['adjusted_contact_type'] != 'UnrecordedBatting' else [])
        actual_direction = direction(event['factorial'][7]) if event['factorial'] else event['status']
        for scope in scopes:
            for stage,a,b in strata(r, actual_direction):
                key = (scope,r['variant'],stage,a,b)
                group = self.summary[key]; group['states'] += 1
                if event['descriptors'] is None:continue
                group['eligible_contacts'] += 1
                group['fair_contacts'] += direction(event['factorial'][7]) == 'fair_direction'
                for k,v in event['descriptors'].items():
                    if v is not None:
                        group[k] += v; group['n_'+k] += 1
                for mask,m in event['factorial'].items():
                    f = self.factorial[key+(mask,)]; f['eligible_contacts'] += 1
                    if m['collision_status'] != 'impulse_contact':continue
                    f['impulse_contacts'] += 1
                    f['fair_contacts'] += direction(m) == 'fair_direction'
                    for k in OUTCOMES:f[k] += m[k]
                if event['contributions'] is None:continue
                for outcome, contributions in event['contributions'].items():
                    p = self.attribution[key+(outcome,)]; p['complete_contacts'] += 1
                    p['reference'] += event['factorial'][0][outcome]; p['actual'] += event['factorial'][7][outcome]
                    for name,value in contributions.items():
                        p[name] += value
                        p[name+'_negative'] += value < -1e-10
                        p[name+'_positive'] += value > 1e-10
        if event['descriptors'] is not None and len(self.examples) < self.sample_limit:
            sample = dict(game_id=r['game_id'],count_seq=r['count_seq'],variant=r['variant'],
                status=event['status'],**event['descriptors'])
            for outcome in OUTCOMES:
                for factor in FACTORS:
                    sample[outcome+'_'+factor] = event['contributions'][outcome][factor] if event['contributions'] else None
            self.examples.append(sample)

    def write(self, output):
        rows = []
        for key,g in sorted(self.summary.items()):
            row = dict(zip(KEYS,key), states=g['states'],eligible_contacts=g['eligible_contacts'],
                unavailable_states=g['states']-g['eligible_contacts'],fair_contacts=g['fair_contacts'])
            for k in DESCRIPTORS:
                row['n_'+k] = g['n_'+k]; row['avg_'+k] = g[k]/g['n_'+k] if g['n_'+k] else None
            rows.append(row)
        write_csv(output/'axial_response_summary.csv',KEYS+['states','eligible_contacts','unavailable_states','fair_contacts']+
            [prefix+k for k in DESCRIPTORS for prefix in ('n_','avg_')],rows)
        rows = []
        for key,g in sorted(self.factorial.items()):
            mask = key[-1]
            row = dict(zip(KEYS,key[:-1]), mask=mask,local_velocity='actual' if mask&1 else 'reference',
                rigid_recoil='actual' if mask&2 else 'reference',positional_e='profile' if mask&4 else '0.5',
                eligible_contacts=g['eligible_contacts'],impulse_contacts=g['impulse_contacts'],
                nonapproaching_contacts=g['eligible_contacts']-g['impulse_contacts'],fair_contacts=g['fair_contacts'])
            for k in OUTCOMES:row['avg_'+k] = g[k]/g['impulse_contacts'] if g['impulse_contacts'] else None
            rows.append(row)
        write_csv(output/'axial_response_factorial.csv',KEYS+['mask','local_velocity','rigid_recoil','positional_e',
            'eligible_contacts','impulse_contacts','nonapproaching_contacts','fair_contacts']+['avg_'+k for k in OUTCOMES],rows)
        rows = []
        for key,g in sorted(self.attribution.items()):
            n = g['complete_contacts']; row = dict(zip(KEYS+['outcome'],key),complete_contacts=n,
                avg_reference=g['reference']/n,avg_actual=g['actual']/n,avg_delta=(g['actual']-g['reference'])/n)
            for name in FACTORS:
                row['avg_'+name] = g[name]/n
                row[name+'_negative_contacts'] = g[name+'_negative']; row[name+'_positive_contacts'] = g[name+'_positive']
            rows.append(row)
        write_csv(output/'axial_response_attribution.csv',KEYS+['outcome','complete_contacts','avg_reference','avg_actual','avg_delta']+
            [k for name in FACTORS for k in ('avg_'+name,name+'_negative_contacts',name+'_positive_contacts')],rows)
        keys = ['variant','status']
        write_csv(output/'axial_response_status.csv',keys+['states'],[dict(zip(keys,k),states=n) for k,n in sorted(self.status.items())])
        fields = ['game_id','count_seq','variant','status']+list(DESCRIPTORS)+[o+'_'+f for o in OUTCOMES for f in FACTORS]
        write_csv(output/'axial_response_examples.csv',fields,self.examples)


def run(input_dir, output, sample_limit=500):
    if not isinstance(sample_limit,int) or sample_limit < 0:raise ValueError('invalid_sample_limit')
    source = ContactSource(input_dir); output = Path(output); output.mkdir(parents=True,exist_ok=False)
    checks = defaultdict(float); counts = Counter(); aggregate = Aggregate(sample_limit)
    for r in source.rows(checks):
        result = event(r,checks); aggregate.add(r,result)
        counts['trial_rows'] += 1; counts['source_geometry_contacts'] += r['status'] == 'contact'
        counts[result['status']] += 1
    aggregate.write(output)
    coverage = {k:counts[k] for k in ('trial_rows','source_geometry_contacts','eligible',
        'eligible:counterfactual_nonapproaching','not_evaluated:unsupported_cap','not_evaluated:out_of_curve',
        'not_evaluated:non_approaching','not_evaluated:no_contact','not_evaluated:overlap_at_start','not_evaluated:unresolved')}
    write_csv(output/'axial_response_coverage.csv',list(coverage),[coverage])
    write_csv(output/'axial_response_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    metadata = dict(version=1,input_sha256=source.hashes,input_metadata=source.meta,bat=profile.metadata(),
        factors=FACTORS,reference_saved_axial_m=0.,reference_e=.5,sample_limit=sample_limit,
        assumptions=[
            'Reference velocity = saved point velocity - omega cross (axis * saved axial). Incoming velocity and contact normal are frozen.',
            'Reference recoil uses s=0 of the same Nathan bat, not constant 0.6kg; actual recoil uses saved s.',
            'Eight algebraic normalizations are not eight realizable swings with one fixed center-of-mass trajectory.',
            'Signed Shapley attribution averages all six factor orders; effects sum to actual minus reference. Not independent physical losses.',
            'Attribution uses only contacts approaching in all eight normalizations. Factorial means have their own impulse denominators.',
            'Actual world-frame energy budget separates ball energy, bat rigid energy and lumped effective loss. No measured vibration or hand/body energy.',
            'Published e_eff already lumps ball deformation and bat vibration effects; no extra vibration/body loss is subtracted.',
            'Source z mapping and [0.54,0.84]m interpolation domain are inherited; near-grip impacts are not simulated or extrapolated.',
            'Overall/source_z averages include all directions; source_z_direction fixes membership using actual mask-7 direction for every factorial mask.',
            'Scopes, variants, stages and factorial masks overlap; do not sum as independent trials.',
            'Examples are the first eligible rows, not a random or representative sample.'])
    (output/'axial_response_metadata.json').write_text(json.dumps(metadata,indent=2,allow_nan=False)+'\n')
    return coverage


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,required=True);p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--sample-limit',type=int,default=500);a=p.parse_args()
    try:print(run(a.input_dir,a.output_dir,a.sample_limit))
    except (OSError,ValueError,sqlite3.Error) as exc:p.exit(1,f'Diagnostic failed: {exc}\n')
