#!/usr/bin/env python3
"""Frozen-contact comparison of rigid recoil and a published positional e_eff template."""
import argparse
import json
import sqlite3
from collections import Counter, defaultdict
from pathlib import Path

import bat_vibration_profile as profile
from bat_mass_presets import measured_wood
from contact_3d import ContactGeometry, band
from contact_collision_probe import write_csv
from contact_recoil import inverse_mass, solve, recoil_checks
from effective_mass_probe import evaluate, direction, strata as mass_strata
from forecast_contact_geometry import vec
from observation_contact_source import ContactSource

CONSTANT='constant_0.6kg'
OLD='kensrud_rigid_0.5'
RIGID='nathan_rigid_0.5'
FULL='nathan_fig12_full_speed'
HALF='nathan_fig12_half_speed'
MODELS=(CONSTANT,OLD,RIGID,FULL,HALF)
CONTRASTS=((OLD,RIGID),(RIGID,FULL),(RIGID,HALF))
KEYS=['scope','variant','stage','band_1','band_2']
METRICS=['applied_e','ev_ms','vla_deg','effective_mass_kg','source_speed_ratio']


def missing(reason):
    return dict(collision_status=reason,**{k:None for k in METRICS})


def event(r,checks):
    old=measured_wood()
    results=evaluate(r,[(CONSTANT,'constant',.25),(OLD,'free',old['gyration_radius_m'])],
        old['body_mass_kg'],old['cm_offset_from_reference_m'],checks)
    for m in results.values():
        m.update(applied_e=.5 if m['collision_status']=='impulse_contact' else None,source_speed_ratio=None)
    z=profile.REFERENCE_Z_M+r['closest_axial_at_entry_m'] if r['status']=='contact' else None
    r['_source_z_m']=z;r['_source_speed_ratio']=None
    if r['status']!='contact':reason='not_evaluated:'+r['status']
    elif r['collision_status']!='impulse_contact':reason='not_evaluated:non_approaching'
    elif r['contact_region']!='side':reason='not_evaluated:unsupported_cap'
    elif profile.profile_e(z) is None:reason='not_evaluated:out_of_curve'
    else:reason='eligible'
    if reason!='eligible':
        results.update({name:missing(reason) for name in (RIGID,FULL,HALF)})
        return results,reason
    axis=vec(r,'axis_at_entry_');n=vec(r,'normal_');vin=vec(r,'ball_velocity_');vbat=vec(r,'point_velocity_')
    g=ContactGeometry(r['entry_since_release_sec'],axis,n,r['axial_at_entry_m'],r['closest_axial_at_entry_m'],r['half_length_m'],'side')
    inv,arm,inertia=inverse_mass(axis,n,r['closest_axial_at_entry_m'],r['lever_m'],'free',
        profile.MASS_KG,profile.GYRATION_M,profile.CM_Z_M-profile.REFERENCE_Z_M)
    omega=tuple(float(r['omega_'+a+'_rad_s']) for a in 'xyz')
    expected=64+51*(z-profile.CM_Z_M)
    ratio=-r['relative_normal_ms']/expected;r['_source_speed_ratio']=ratio
    for name,e,source_ratio in ((RIGID,.5,None),(FULL,profile.profile_e(z),ratio),(HALF,profile.profile_e(z,'half_speed'),2*ratio)):
        m=solve(g,vin,vbat,inv,e);m.update(applied_e=e,source_speed_ratio=source_ratio)
        for key,error in recoil_checks(m,vin,vbat,omega,n,arm,inertia,'free',profile.MASS_KG,e).items():
            checks['profile_'+key]=max(checks['profile_'+key],error)
            if error>1e-7:raise ValueError('profile_recoil_mismatch:'+key)
        results[name]=m
    return results,reason


def strata(r):
    yield from mass_strata(r)
    yield 'source_z',band(r['_source_z_m'],(.60,.64,.68,.72,.76,.80,.84)),r['contact_region'] or 'no_entry'
    yield 'normal_source_speed_ratio',band(r['_source_speed_ratio'],(.5,.75,1,1.25,1.5)),'all'


def ev_band(m):
    return 'not_evaluated' if m['ev_ms'] is None else '<25' if m['ev_ms']<25 else '25..<40' if m['ev_ms']<40 else '40+'


class Aggregate:
    def __init__(self):self.groups=defaultdict(Counter);self.pairs=defaultdict(Counter);self.transitions=Counter()
    def add(self,r,results):
        scopes=['all_swings']+(['recorded_batting_only'] if r['adjusted_contact_type']!='UnrecordedBatting' else [])
        for scope in scopes:
            for stage,a,b in strata(r):
                key=(scope,r['variant'],stage,a,b)
                for name,m in results.items():
                    g=self.groups[key+(name,)];g['states']+=1;g['impulse_contacts']+=m['collision_status']=='impulse_contact'
                    fair=direction(m)=='fair_direction';g['fair_contacts']+=fair
                    g['foul_contacts']+=direction(m)=='foul_direction';g['undefined_direction_contacts']+=direction(m)=='undefined_direction'
                    for metric in METRICS:
                        if m.get(metric) is not None:g[metric]+=m[metric];g[metric+'_n']+=1
                    if fair:
                        g['fair_ev']+=m['ev_ms'];g['weak_fair']+=m['ev_ms']<25;g['strong_fair']+=m['ev_ms']>=40
                for before,after in CONTRASTS:
                    left,right=results[before],results[after];p=self.pairs[key+(before,after)];p['states']+=1
                    ld,rd=direction(left),direction(right)
                    self.transitions[key+(before,after,ld,ev_band(left),rd,ev_band(right))]+=1
                    if left['collision_status']!=right['collision_status'] or left['collision_status']!='impulse_contact':continue
                    p['paired_impulse_contacts']+=1;p['before_ev']+=left['ev_ms'];p['after_ev']+=right['ev_ms']
                    if ld=='fair_direction' and rd=='fair_direction':
                        p['paired_fair_contacts']+=1;p['before_fair_ev']+=left['ev_ms'];p['after_fair_ev']+=right['ev_ms']
                    if ld=='fair_direction' and left['ev_ms']>=40:
                        p['before_strong_fair']+=1
                        dest='nonfair' if rd!='fair_direction' else 'weak' if right['ev_ms']<25 else 'strong' if right['ev_ms']>=40 else 'medium'
                        p['strong_to_'+dest]+=1

    def write(self,output):
        keys=KEYS+['model'];rows=[]
        for key,g in sorted(self.groups.items()):
            fair=g['fair_contacts'];n=g['impulse_contacts']
            r=dict(zip(keys,key),states=g['states'],impulse_contacts=n,unavailable_or_nonapproaching_states=g['states']-n,
                fair_contacts=fair,foul_contacts=g['foul_contacts'],undefined_direction_contacts=g['undefined_direction_contacts'],
                weak_fair_contacts=g['weak_fair'],strong_fair_contacts=g['strong_fair'],
                avg_fair_ev_ms=g['fair_ev']/fair if fair else None,
                weak_pct_of_fair=100*g['weak_fair']/fair if fair else None,strong_pct_of_fair=100*g['strong_fair']/fair if fair else None)
            for metric in METRICS:
                r['n_'+metric]=g[metric+'_n'];r['avg_'+metric]=g[metric]/g[metric+'_n'] if g[metric+'_n'] else None
            rows.append(r)
        fields=keys+['states','impulse_contacts','unavailable_or_nonapproaching_states','fair_contacts','foul_contacts','undefined_direction_contacts',
            'weak_fair_contacts','strong_fair_contacts','avg_fair_ev_ms','weak_pct_of_fair','strong_pct_of_fair']+[p+k for k in METRICS for p in ('n_','avg_')]
        write_csv(output/'vibration_profile_summary.csv',fields,rows)
        rows=[];keys=KEYS+['before_model','after_model']
        for key,g in sorted(self.pairs.items()):
            n=g['paired_impulse_contacts'];fair=g['paired_fair_contacts'];strong=g['before_strong_fair']
            r=dict(zip(keys,key),states=g['states'],paired_impulse_contacts=n,paired_fair_contacts=fair,before_strong_fair_contacts=strong)
            for label in ('weak','medium','strong','nonfair'):r['strong_to_'+label+'_contacts']=g['strong_to_'+label]
            r['strong_retained_pct']=100*g['strong_to_strong']/strong if strong else None
            for moment in ('before','after'):
                r[moment+'_paired_ev_ms']=g[moment+'_ev']/n if n else None
                r[moment+'_paired_fair_ev_ms']=g[moment+'_fair_ev']/fair if fair else None
            rows.append(r)
        fields=keys+['states','paired_impulse_contacts','paired_fair_contacts','before_strong_fair_contacts']+[
            'strong_to_'+k+'_contacts' for k in ('weak','medium','strong','nonfair')]+['strong_retained_pct','before_paired_ev_ms','before_paired_fair_ev_ms','after_paired_ev_ms','after_paired_fair_ev_ms']
        write_csv(output/'vibration_profile_pairs.csv',fields,rows)
        keys+=['before_direction','before_ev_band','after_direction','after_ev_band']
        write_csv(output/'vibration_profile_transitions.csv',keys+['states'],[dict(zip(keys,k),states=n) for k,n in sorted(self.transitions.items())])


def run(input_dir,output):
    source=ContactSource(input_dir);output=Path(output);output.mkdir(parents=True,exist_ok=False)
    checks=defaultdict(float);counts=Counter();status=Counter();agg=Aggregate()
    for r in source.rows(checks):
        results,eligibility=event(r,checks);agg.add(r,results)
        counts['trial_rows']+=1;counts['source_geometry_contacts']+=r['status']=='contact'
        counts['source_impulse_contacts']+=r['collision_status']=='impulse_contact'
        counts['non_approaching_contacts']+=eligibility=='not_evaluated:non_approaching'
        counts['eligible_side_contacts']+=eligibility=='eligible'
        counts['unsupported_cap_contacts']+=eligibility=='not_evaluated:unsupported_cap'
        counts['out_of_curve_contacts']+=eligibility=='not_evaluated:out_of_curve'
        for name,m in results.items():status[r['variant'],name,eligibility,m['collision_status']]+=1
    agg.write(output)
    coverage={k:counts[k] for k in ('trial_rows','source_geometry_contacts','source_impulse_contacts','non_approaching_contacts',
        'eligible_side_contacts','unsupported_cap_contacts','out_of_curve_contacts')}
    coverage['models']=len(MODELS)
    write_csv(output/'vibration_profile_coverage.csv',list(coverage),[coverage])
    write_csv(output/'vibration_profile_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    keys=['variant','model','profile_eligibility','collision_status']
    write_csv(output/'vibration_profile_status.csv',keys+['states'],[dict(zip(keys,k),states=n) for k,n in sorted(status.items())])
    write_csv(output/'vibration_profile_knots.csv',['z_from_knob_m','full_speed_e_eff','half_speed_e_eff'],
        [dict(zip(['z_from_knob_m','full_speed_e_eff','half_speed_e_eff'],p)) for p in profile.KNOTS])
    (output/'vibration_profile_metadata.json').write_text(json.dumps(dict(version=1,input_sha256=source.hashes,input_metadata=source.meta,
        source_profile=profile.metadata(),old_mass_preset=measured_wood(),models=MODELS,contrasts=CONTRASTS,
        speed_ratio='Actual normal approach speed / published local impact speed; diagnostic strata, not a speed interpolation or eligibility criterion.',
        speed_ratio_strata='Band membership always uses the full-speed denominator; half-speed model average source_speed_ratio is twice that value.',
        scope='Offline 3D side-contact extension of a published 1D curve; no game/search/RNG changes. Curves are coarse model templates, not fitted game coefficients.'),indent=2)+'\n')
    return coverage


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--input-dir',type=Path,required=True);p.add_argument('--output-dir',type=Path,required=True);a=p.parse_args()
    try:print(run(a.input_dir,a.output_dir))
    except (OSError,ValueError,sqlite3.Error) as exc:p.exit(1,f'Diagnostic failed: {exc}\n')
