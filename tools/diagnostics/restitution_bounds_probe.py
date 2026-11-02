#!/usr/bin/env python3
"""Can lower normal restitution produce weak fair balls on the same saved contacts?"""
import argparse
import csv
import json
import sqlite3
from collections import Counter, defaultdict
from pathlib import Path

from bat_mass_presets import PRESET, measured_wood
from contact_3d import ContactGeometry
from contact_collision_probe import write_csv
from contact_recoil import inverse_mass, solve, recoil_checks
from effective_mass_probe import evaluate, strata
from forecast_contact_geometry import vec
from observation_contact_source import ContactSource
from restitution_bounds import analyze

FLAGS=['fair_exists','baseline_fair','baseline_weak_fair','weak_fair_possible','possible_only_after_reduction','e0_fair']
METRICS=['baseline_ev_ms','e0_ev_ms','tangent_floor_ms','min_any_ev_ms','fair_ev_infimum_ms','fair_infimum_e']
BOUNDS=list(analyze((0.,-40.,0.),(0.,35.,0.),(0.,1.,0.),1/.6))
KEYS=['scope','variant','stage','band_1','band_2']
IDS=['game_id','count_seq','variant','forecast_condition','lever_m','adjusted_contact_type','contact_region','search_status','collision_status']


class Aggregate:
    def __init__(self):self.groups=defaultdict(Counter);self.diagnoses=Counter()
    def add(self,r,m):
        scopes=['all_swings']+(['recorded_batting_only'] if r['adjusted_contact_type']!='UnrecordedBatting' else [])
        for scope in scopes:
            for stage,a,b in strata(r):
                key=(scope,r['variant'],stage,a,b);g=self.groups[key]
                g['states']+=1;g['geometry_contacts']+=r['status']=='contact'
                g['impulse_contacts']+=r['collision_status']=='impulse_contact'
                self.diagnoses[key+(m['diagnosis'],)]+=1
                for flag in FLAGS:g[flag]+=m[flag] or 0
                if m['baseline_fair'] and m['weak_fair_possible']:g['baseline_fair_weak_possible']+=1
                if m['fair_min_attained'] is not None:
                    g['fair_min_attained']+=m['fair_min_attained'];g['fair_infimum_only']+=not m['fair_min_attained']
                for k in METRICS:
                    if m[k] is not None:g[k]+=m[k];g[k+'_n']+=1
    def write(self,output):
        rows=[]
        for key,g in sorted(self.groups.items()):
            r=dict(zip(KEYS,key),states=g['states'],geometry_contacts=g['geometry_contacts'],impulse_contacts=g['impulse_contacts'])
            for k in FLAGS+['baseline_fair_weak_possible','fair_min_attained','fair_infimum_only']:r[k+'_contacts']=g[k]
            for k in METRICS:
                r['n_'+k]=g[k+'_n'];r['avg_'+k]=g[k]/g[k+'_n'] if g[k+'_n'] else None
            r['weak_possible_pct_of_impulse']=100*g['weak_fair_possible']/g['impulse_contacts'] if g['impulse_contacts'] else None
            r['weak_possible_pct_of_baseline_fair']=100*g['baseline_fair_weak_possible']/g['baseline_fair'] if g['baseline_fair'] else None
            rows.append(r)
        fields=KEYS+['states','geometry_contacts','impulse_contacts']+[k+'_contacts' for k in FLAGS+[
            'baseline_fair_weak_possible','fair_min_attained','fair_infimum_only']]+[p+k for k in METRICS for p in ('n_','avg_')]+[
            'weak_possible_pct_of_impulse','weak_possible_pct_of_baseline_fair']
        write_csv(output/'restitution_bounds_summary.csv',fields,rows)
        write_csv(output/'restitution_bounds_diagnoses.csv',KEYS+['diagnosis','states'],
            [dict(zip(KEYS+['diagnosis'],key),states=n) for key,n in sorted(self.diagnoses.items())])


def event(r,preset,checks):
    configs=[('constant_0.6kg','constant',.25),('free','free',preset['gyration_radius_m'])]
    result=evaluate(r,configs,preset['body_mass_kg'],preset['cm_offset_from_reference_m'],checks)['free']
    if result['collision_status']!='impulse_contact':
        return {k:('not_evaluated:'+r['status'] if k=='diagnosis' and r['status']!='contact' else
                   'not_evaluated:non_approaching' if k=='diagnosis' else None) for k in BOUNDS}
    vin=vec(r,'ball_velocity_');vbat=vec(r,'point_velocity_');n=vec(r,'normal_')
    inv=result['inverse_effective_mass_kg_inv'];m=analyze(vin,vbat,n,inv)
    geometry=ContactGeometry(r['entry_since_release_sec'],vec(r,'axis_at_entry_'),n,r['axial_at_entry_m'],
                             r['closest_axial_at_entry_m'],r['half_length_m'],r['contact_region'])
    _,arm,inertia=inverse_mass(geometry.axis_world,n,r['closest_axial_at_entry_m'],r['lever_m'],'free',
        preset['body_mass_kg'],preset['gyration_radius_m'],preset['cm_offset_from_reference_m'])
    omega=tuple(float(r['omega_'+a+'_rad_s']) for a in 'xyz')
    # Reuse the collision solver independently at both endpoints and an interior witness.
    for e in (0.,.5):
        solved=solve(geometry,vin,vbat,inv,e)
        label='e0' if e==0 else 'baseline'
        error=abs(solved['ev_ms']-m[label+'_ev_ms']);checks['bounds_endpoint_ev_ms']=max(checks['bounds_endpoint_ev_ms'],error)
        if error>1e-7:raise ValueError('bounds_endpoint_ev_mismatch')
        for key,error in recoil_checks(solved,vin,vbat,omega,n,arm,inertia,'free',preset['body_mass_kg'],e).items():
            checks['bounds_'+key]=max(checks['bounds_'+key],error)
            if error>1e-7:raise ValueError('bounds_recoil_mismatch:'+key)
    if m['weak_fair_possible']:
        e=(m['weak_e_lo']+m['weak_e_hi'])/2
        witness=solve(geometry,vin,vbat,inv,e)
        # Allow only numerical verification tolerance; interval endpoints remain strictly open.
        if witness['ev_ms']>=25.+1e-9 or not abs(witness['hla_deg'])<45.+1e-9:
            raise ValueError('weak_interval_witness_mismatch')
    return m


def run(input_dir,output):
    source=ContactSource(input_dir);output=Path(output);output.mkdir(parents=True,exist_ok=False)
    preset=measured_wood();checks=defaultdict(float);counts=Counter();agg=Aggregate()
    with (output/'restitution_bounds_contacts.csv').open('w',newline='') as f:
        writer=csv.DictWriter(f,fieldnames=IDS+BOUNDS);writer.writeheader()
        for r in source.rows(checks):
            m=event(r,preset,checks);agg.add(r,m)
            ids={k:r[k] for k in IDS if k not in ('search_status',)};ids['search_status']=r['status']
            writer.writerow(dict(ids,**m));counts['trial_rows']+=1
            counts['geometry_contacts']+=r['status']=='contact';counts['impulse_contacts']+=r['collision_status']=='impulse_contact'
            counts['weak_fair_possible']+=m['weak_fair_possible'] or 0
    agg.write(output)
    coverage={k:counts[k] for k in ('trial_rows','geometry_contacts','impulse_contacts','weak_fair_possible')}
    write_csv(output/'restitution_bounds_coverage.csv',list(coverage),[coverage])
    write_csv(output/'restitution_bounds_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    (output/'restitution_bounds_metadata.json').write_text(json.dumps(dict(version=1,input_sha256=source.hashes,
        input_metadata=source.meta,bat_preset=preset,mass_model='free_'+PRESET,restitution_range=[0,.5],weak_ev_threshold_ms=25.,
        fair_definition='outgoing_y > abs(outgoing_x), strictly; both +/-45-degree boundaries excluded',
        solver='analytic affine velocity, linear fair constraints and convex quadratic speed; no e grid',
        infimum='Open fair endpoints are limits, not attained fair minima; check fair_min_attained',
        note='Frozen saved geometry and velocities; no search, RNG, game outcome or coefficient changes. Aggregate variants/stages/scopes are overlapping views, not additional unique swings.'),indent=2)+'\n')
    return coverage


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--input-dir',type=Path,required=True)
    p.add_argument('--output-dir',type=Path,required=True);a=p.parse_args()
    try:print(run(a.input_dir,a.output_dir))
    except (OSError,ValueError,sqlite3.Error) as exc:p.exit(1,f'Diagnostic failed: {exc}\n')
