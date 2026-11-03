#!/usr/bin/env python3
"""Compare normal, centerline-spin and coupled finite-barrel responses on frozen contacts."""
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
from finite_barrel_collision import solve
from forecast_contact_geometry import vec
from ground_spin_probe import check_metrics
from observation_contact_source import ContactSource
from rotating_bat_probe import norm, sub

KEYS = ['scope','variant','stage','band_1','band_2']
METRICS = ['incoming_spin_rpm','incoming_spin_x_rad_s','incoming_spin_y_rad_s','incoming_spin_z_rad_s','ev_ms','vla_deg','spin_rpm','spin_x_rad_s','spin_y_rad_s','spin_z_rad_s',
    'ball_translation_before_j','ball_translation_after_j','ball_rotation_before_j','ball_rotation_after_j',
    'bat_before_j','bat_after_j','system_before_j','system_after_j','total_loss_j',
    'normal_impulse_kg_ms','tangent_impulse_kg_ms','bat_axial_spin_after_rad_s',
    'surface_speed_ms','centerline_speed_ms','barrel_radius_m','axial_inertia_kg_m2',
    'normal_axial_coupling_kg_inv','post_tangent_slip_ms','tangent_target_residual_ms',
    'first_ground_distance_m','post_ground_horizontal_ms','post_ground_spin_rpm','rebound_height_m']
PAIR_METRICS = ['incoming_spin_rpm','incoming_spin_x_rad_s','incoming_spin_y_rad_s','incoming_spin_z_rad_s','ev_ms','vla_deg','spin_rpm','ball_translation_after_j','ball_rotation_after_j',
    'post_ground_horizontal_ms','rebound_height_m']


def configs(inertias):
    result = [dict(name='normal_profile',kind='normal',mu=0.,et=0.),
        dict(name='centerline_et0',kind='centerline',mu=.2,et=0.),
        dict(name='centerline_et0.4',kind='centerline',mu=.2,et=.4),
        dict(name='finite_mu0',kind='finite',mu=0.,et=0.,axial_inertia=inertias[0])]
    for inertia in inertias:
        for et in (0.,.4):
            result.append(dict(name=f'finite_I{inertia:g}_et{et:g}',kind='finite',mu=.2,et=et,axial_inertia=inertia))
    return result


def contrasts(models):
    yield 'normal_profile','finite_mu0'
    for m in models:
        if m['kind']=='finite' and m['mu']>0:
            yield f"centerline_et{m['et']:g}",m['name']


def event(r,checks,models,ball_spin,height,spin_source=None,invalid_spin_policy='error'):
    old,eligibility = baseline.event(r,checks)
    if eligibility!='eligible':
        return {m['name']:dict(collision_status=eligibility,ev_ms=None) for m in models},eligibility
    if spin_source is not None:
        from pitch_spin_source import InvalidPitchSpin
        try:ball_spin=spin_source.get(r)
        except InvalidPitchSpin:
            if invalid_spin_policy!='exclude':raise
            return {m['name']:dict(collision_status='not_evaluated:invalid_pitch_spin',ev_ms=None) for m in models},'not_evaluated:invalid_pitch_spin'

    g = ContactGeometry(r['entry_since_release_sec'],vec(r,'axis_at_entry_'),vec(r,'normal_'),
        r['axial_at_entry_m'],r['closest_axial_at_entry_m'],r['half_length_m'],'side')
    vin,vbat = vec(r,'ball_velocity_'),vec(r,'point_velocity_')
    omega = tuple(float(r['omega_'+a+'_rad_s']) for a in 'xyz')
    radius = r['effective_radius_m']-BALL_RADIUS_M
    if radius<=0:raise ValueError('nonpositive_derived_barrel_radius')
    inertia = profile.MASS_KG*profile.GYRATION_M**2;cm = profile.CM_Z_M-profile.REFERENCE_Z_M
    e = profile.profile_e(r['_source_z_m']);results = {}
    for model in models:
        name = model['name']
        if model['kind']=='normal':
            m = dict(old[baseline.FULL],outgoing_spin_rad_s=ball_spin,spin_rpm=norm(ball_spin)*30/math.pi)
        elif model['kind']=='centerline':
            m = solve_side(g,vin,vbat,ball_spin,omega,profile.MASS_KG,inertia,cm,e,model['mu'],model['et'])
        else:
            m = solve(g,vin,vbat,ball_spin,omega,profile.MASS_KG,inertia,model['axial_inertia'],cm,radius,e,model['mu'],model['et'])
        check_metrics(checks,'collision_',m.get('checks',{}))
        if m['collision_status']=='impulse_contact':
            m['incoming_spin_rpm']=norm(ball_spin)*30/math.pi
            for axis,value in zip('xyz',ball_spin):m['incoming_spin_'+axis+'_rad_s']=value
            for axis,value in zip('xyz',m['outgoing_spin_rad_s']):m['spin_'+axis+'_rad_s'] = value
            ground = first_ground(m['outgoing_world_ms'],m['outgoing_spin_rad_s'],height)
            check_metrics(checks,'', {k:ground[k] for k in ('ground_energy_balance_j','ground_negative_dissipation_j','flight_energy_balance_j')})
            m.update(ground)
        results[name] = m
    # A finite-radius normal force acts along n, so both surface correction and
    # extra radius torque vanish in the normal response (same input geometry).
    control = results['finite_mu0'];normal = results['normal_profile']
    if control['collision_status']!='impulse_contact':raise ValueError('finite_normal_control_unavailable')
    check_metrics(checks,'control_',dict(outgoing_ms=norm(sub(control['outgoing_world_ms'],normal['outgoing_world_ms'])),
        spin_rad_s=norm(sub(control['outgoing_spin_rad_s'],ball_spin))))
    return results,eligibility


def strata(r,reference_direction):
    yield 'overall','all','all'
    yield 'source_z',band(r['_source_z_m'],(.60,.64,.68,.72,.76,.80,.84)),r['contact_region'] or 'no_entry'
    yield 'source_z_reference_direction',band(r['_source_z_m'],(.60,.64,.68,.72,.76,.80,.84)),reference_direction
    yield 'baseline_impact',band(r['local_abs_b_over_r'],(.25,.5,.75,.95)),band(r['normal_speed_fraction'],(.5,.8,.95))


def accumulate_pair(p,left,right):
    p['states']+=1
    if left['collision_status']!='impulse_contact' or right['collision_status']!='impulse_contact':return
    p['paired_contacts']+=1
    ld,rd=direction(left),direction(right)
    p['paired_fair_contacts']+=ld==rd=='fair_direction'
    p['before_fair_after_nonfair']+=ld=='fair_direction' and rd!='fair_direction'
    p['before_nonfair_after_fair']+=ld!='fair_direction' and rd=='fair_direction'
    for metric in PAIR_METRICS:
        if left.get(metric) is None or right.get(metric) is None:continue
        p['n_'+metric]+=1;p['before_'+metric]+=left[metric];p['after_'+metric]+=right[metric]
        p['delta_'+metric]+=right[metric]-left[metric]


def add_spin_pairs(groups,r,zero,spun):
    scopes=['all_swings']+(['recorded_batting_only'] if r['adjusted_contact_type']!='UnrecordedBatting' else [])
    for scope in scopes:
        for stage,a,b in strata(r,direction(zero['normal_profile'])):
            for model in zero:
                key=(scope,r['variant'],stage,a,b,model+':zero',model+':input_spin')
                accumulate_pair(groups[key],zero[model],spun[model])


def write_pair_groups(output,filename,groups):
    counts=['states','paired_contacts','paired_fair_contacts','before_fair_after_nonfair','before_nonfair_after_fair']
    rows=[]
    for key,p in sorted(groups.items()):
        row=dict(zip(KEYS+['before_model','after_model'],key),**{k:p[k] for k in counts})
        for metric in PAIR_METRICS:
            n=p['n_'+metric];row['n_'+metric]=n
            for label in ('before','after','delta'):row['avg_'+label+'_'+metric]=p[label+'_'+metric]/n if n else None
        rows.append(row)
    write_csv(output/filename,KEYS+['before_model','after_model']+counts+
        [prefix+k for k in PAIR_METRICS for prefix in ('n_','avg_before_','avg_after_','avg_delta_')],rows)


class Aggregate:
    def __init__(self,models,sample_limit):
        self.summary=defaultdict(Counter);self.pairs=defaultdict(Counter);self.status=Counter()
        self.contrasts=list(contrasts(models));self.examples=[];self.sample_limit=sample_limit

    def add(self,r,results):
        scopes=['all_swings']+(['recorded_batting_only'] if r['adjusted_contact_type']!='UnrecordedBatting' else [])
        for name,m in results.items():
            self.status[r['variant'],name,m['collision_status'],m.get('tangent_regime','not_applicable')]+=1
        for scope in scopes:
            for stage,a,b in strata(r,direction(results['normal_profile'])):
                key=(scope,r['variant'],stage,a,b)
                for name,m in results.items():
                    g=self.summary[key+(name,)];g['states']+=1
                    if m['collision_status']!='impulse_contact':continue
                    g['impulse_contacts']+=1;g['fair_contacts']+=direction(m)=='fair_direction'
                    g['coulomb_limited_contacts']+=m.get('tangent_regime')=='coulomb_limited'
                    for metric in METRICS:
                        if m.get(metric) is not None:g[metric]+=m[metric];g['n_'+metric]+=1
                for before,after in self.contrasts:
                    accumulate_pair(self.pairs[key+(before,after)],results[before],results[after])
        for name,m in results.items():
            if len(self.examples)>=self.sample_limit:break
            if m['collision_status']!='impulse_contact':continue
            self.examples.append(dict(game_id=r['game_id'],count_seq=r['count_seq'],variant=r['variant'],
                source_z_m=r['_source_z_m'],model=name,tangent_regime=m.get('tangent_regime'),
                **{k:m.get(k) for k in METRICS}))

    def write(self,output):
        counts=['states','impulse_contacts','fair_contacts','coulomb_limited_contacts']
        rows=[]
        for key,g in sorted(self.summary.items()):
            row=dict(zip(KEYS+['model'],key),**{k:g[k] for k in counts},unavailable_states=g['states']-g['impulse_contacts'])
            for k in METRICS:row['n_'+k]=g['n_'+k];row['avg_'+k]=g[k]/g['n_'+k] if g['n_'+k] else None
            rows.append(row)
        write_csv(output/'finite_barrel_summary.csv',KEYS+['model']+counts+['unavailable_states']+
            [prefix+k for k in METRICS for prefix in ('n_','avg_')],rows)
        write_pair_groups(output,'finite_barrel_pairs.csv',self.pairs)
        keys=['variant','model','collision_status','tangent_regime']
        write_csv(output/'finite_barrel_status.csv',keys+['states'],[dict(zip(keys,k),states=n) for k,n in sorted(self.status.items())])
        write_csv(output/'finite_barrel_examples.csv',['game_id','count_seq','variant','source_z_m','model','tangent_regime']+METRICS,self.examples)


def run(input_dir,output,inertias=(.00015,.0003),ball_spin=(0.,0.,0.),height=.9,sample_limit=500,
        pitch_db=None,gyro_sign=0,invalid_spin_policy='error'):
    if (not inertias or len(set(inertias))!=len(inertias) or any(not math.isfinite(x) or x<=0 for x in inertias)
        or not vector(ball_spin) or not math.isfinite(height) or height<=0
        or not isinstance(sample_limit,int) or sample_limit<0 or gyro_sign not in (-1,0,1)
        or invalid_spin_policy not in ('error','exclude') or (pitch_db is None and invalid_spin_policy!='error')
        or (pitch_db is None and gyro_sign!=0) or (pitch_db is not None and any(ball_spin))):raise ValueError('invalid_probe_parameters')
    source=ContactSource(input_dir);output=Path(output);output.mkdir(parents=True,exist_ok=False)
    models=configs(inertias);checks=defaultdict(float);counts=Counter();aggregate=Aggregate(models,sample_limit);spin_pairs=defaultdict(Counter)
    from pitch_spin_source import PitchSpinSource
    spin_source=PitchSpinSource(pitch_db,gyro_sign,output/'finite_barrel_invalid_spin.csv') if pitch_db is not None else None
    try:
        for r in source.rows(checks):
            results,eligibility=event(r,checks,models,ball_spin,height,spin_source,invalid_spin_policy);aggregate.add(r,results)
            if spin_source is not None or any(ball_spin):
                zero,_=event(r,checks,models,(0.,0.,0.),height)
                add_spin_pairs(spin_pairs,r,zero,results)
            counts['trial_rows']+=1;counts['source_geometry_contacts']+=r['status']=='contact';counts[eligibility]+=1
        spin_metadata=spin_source.metadata() if spin_source is not None else dict(source='constant_world_vector_scenario',vector_rad_s=ball_spin)
    finally:
        if spin_source is not None:spin_source.close()
    aggregate.write(output)
    write_pair_groups(output,'finite_barrel_spin_pairs.csv',spin_pairs)
    coverage={k:counts[k] for k in ('trial_rows','source_geometry_contacts','eligible','not_evaluated:unsupported_cap',
        'not_evaluated:out_of_curve','not_evaluated:invalid_pitch_spin','not_evaluated:non_approaching','not_evaluated:no_contact','not_evaluated:overlap_at_start','not_evaluated:unresolved')}
    coverage['models']=len(models)
    write_csv(output/'finite_barrel_coverage.csv',list(coverage),[coverage])
    write_csv(output/'finite_barrel_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    metadata=dict(version=3,invalid_spin_policy=invalid_spin_policy,input_sha256=source.hashes,input_metadata=source.meta,bat=profile.metadata(),models=models,
        incoming_ball_spin_world_rad_s=ball_spin if pitch_db is None else None,pitch_spin_source=spin_metadata,ground_height_m=height,ground_e=.45,ground_mu=.25,sample_limit=sample_limit,
        assumptions=[
            'Saved contact point velocity is on the centerline. Add omega cross (R*n) for the surface point; infer CM velocity from the centerline point.',
            'R = source effective cylinder radius - assumed sphere radius 0.036m. Same saved geometry/contact normal; no search or timing changes.',
            'Axisymmetric transverse inertia from Nathan; axial inertias are explicitly assumed sensitivities, not Nathan measurements or borrowed bat fits.',
            'All models share the positional e_eff curve and eligibility. No extra vibration/body loss. Coupled total loss cannot be uniquely split into normal/tangent losses.',
            'Terminal impulse target en/et with full normal-axial coupling and circumferential torsion; Coulomb cone is enforced along a ray, not by time integration or exact maximum dissipation.',
            'Energy-producing or tensile terminal targets are explicitly unavailable, never fixed by scaling EV or spin.',
            'Finite mu0 must reproduce the old normal outgoing velocity and incident spin. R=0 limit reproduces centerline tangential solver.',
            'Incoming sphere spin is either a constant scenario or an explicitly labeled reconstruction from saved pitch scalars. Ground height is a scenario absent from the CSV. Ground results use gravity only: no drag/Magnus/fielders/fences/rolling.',
            'The zero-spin counterpart changes collision spin only; it does not recompute the pitch flight or contact search.',
            'spin_pairs compares zero vs input spin on each identical source contact/model, using their common successful subset. Nonzero-spin runs calculate both responses.',
            'Pairs compare common successful contacts only; source_z_reference_direction fixes cohort using normal-profile direction. Other averages include all directions.',
            'Scopes/variants/stages/models overlap. Examples are first eligible model rows, not a representative sample.',
            'Rigid collision consistency checks do not calibrate spin magnitudes or establish real-world weak-ball rates.'])
    (output/'finite_barrel_metadata.json').write_text(json.dumps(metadata,indent=2,allow_nan=False)+'\n')
    return coverage


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,required=True);p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--axial-inertias',type=float,nargs='+',default=(.00015,.0003))
    spin=p.add_mutually_exclusive_group()
    spin.add_argument('--ball-spin-rad-s',type=float,nargs=3,default=None,metavar=('X','Y','Z'))
    spin.add_argument('--pitch-db',type=Path)
    p.add_argument('--invalid-spin-policy',choices=('error','exclude'),default='error')
    p.add_argument('--gyro-sign',type=int,choices=(-1,0,1),default=0)
    p.add_argument('--height-m',type=float,default=.9);p.add_argument('--sample-limit',type=int,default=500);a=p.parse_args()
    try:print(run(a.input_dir,a.output_dir,tuple(a.axial_inertias),tuple(a.ball_spin_rad_s or (0.,0.,0.)),a.height_m,a.sample_limit,a.pitch_db,a.gyro_sign,a.invalid_spin_policy))
    except (OSError,ValueError,sqlite3.Error) as exc:p.exit(1,f'Diagnostic failed: {exc}\n')
