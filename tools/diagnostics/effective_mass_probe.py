#!/usr/bin/env python3
"""Audit saved contact CSV and compare constant/free/pivot normal effective mass."""
import argparse
import csv
import json
import math
import sqlite3
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

from contact_3d import ContactGeometry, band
from contact_collision_probe import write_csv
from contact_point_speed_probe import digest
from contact_recoil import inverse_mass, solve, recoil_checks
from forecast_contact_geometry import REQUIRED, audit, vec, signed_band
from rotating_bat_probe import norm, sub
from bat_mass_presets import PRESET, measured_wood

METRICS=['ev_ms','hla_deg','vla_deg','effective_mass_kg','inverse_effective_mass_kg_inv',
         'normal_impulse_kg_ms','incoming_z_ms','impulse_z_ms','outgoing_z_ms']
KEYS=['scope','variant','mass_model','stage','band_1','band_2']


def models(gyrations):
    return [('constant_0.6kg','constant',.25)]+[(f'{model}_k{k:g}m',model,k) for model in ('free','pivot') for k in gyrations]


def evaluate(r,configs,body_mass,cm_offset,checks):
    if r['status']!='contact':
        return {name:dict(collision_status='not_evaluated:'+r['status'],infinite_effective_mass=0,**{k:None for k in METRICS}) for name,_,_ in configs}
    axis=vec(r,'axis_at_entry_');n=vec(r,'normal_');vin=vec(r,'ball_velocity_');vbat=vec(r,'point_velocity_')
    omega=tuple(float(r['omega_'+a+'_rad_s']) for a in 'xyz')
    if not all(math.isfinite(x) for x in omega):raise ValueError('non_finite_omega')
    if abs(sum(a*w for a,w in zip(axis,omega)))>1e-7:raise ValueError('unsupported_axial_spin')
    geometry=ContactGeometry(r['entry_since_release_sec'],axis,n,r['axial_at_entry_m'],r['closest_axial_at_entry_m'],r['half_length_m'],r['contact_region'])
    results={}
    for name,model,k in configs:
        inv,arm,inertia=inverse_mass(axis,n,r['closest_axial_at_entry_m'],r['lever_m'],model,body_mass,k,cm_offset)
        m=solve(geometry,vin,vbat,inv)
        for key,error in recoil_checks(m,vin,vbat,omega,n,arm,inertia,model,body_mass).items():
            checks[key]=max(checks[key],error)
            if error>1e-7:raise ValueError('recoil_identity_mismatch:'+key)
        results[name]=m
    base=results['constant_0.6kg']
    if base['collision_status']!=r['collision_status']:raise ValueError('baseline_collision_mismatch')
    if base['outgoing_world_ms'] is not None:
        error=norm(sub(base['outgoing_world_ms'],vec(r,'outgoing_velocity_')))
        checks['baseline_outgoing_ms']=max(checks['baseline_outgoing_ms'],error)
        if error>1e-7:raise ValueError('baseline_outgoing_mismatch')
    return results


def direction(m):
    if m['collision_status']!='impulse_contact':return m['collision_status']
    if m['hla_deg'] is None:return 'undefined_direction'
    return 'fair_direction' if abs(m['hla_deg'])<45 else 'foul_direction'


def strata(r):
    yield 'overall','all','all'
    yield 'signed_axial',band(r['closest_axial_at_entry_m'],(-.12,-.06,0,.06,.12)) if r['status']=='contact' else 'undefined',r['contact_region'] or 'no_entry'
    yield 'baseline_impact',band(r['local_abs_b_over_r'],(.25,.5,.75,.95)) if r['local_abs_b_over_r'] is not None else 'cap_or_undefined',band(r['normal_speed_fraction'],(.5,.8,.95))
    yield 'forecast_z',signed_band(r['forecast_error_z_m']),'all'


class Aggregate:
    def __init__(self):self.summary=defaultdict(Counter);self.pairs=defaultdict(Counter);self.transitions=Counter();self.mass_distribution=Counter()
    def add(self,r,results):
        scopes=['all_swings']+(['recorded_batting_only'] if r['adjusted_contact_type']!='UnrecordedBatting' else [])
        for scope in scopes:
            for name,m in results.items():
                label=('no_entry' if m['inverse_effective_mass_kg_inv'] is None else
                       'infinite' if m['infinite_effective_mass'] else band(m['effective_mass_kg'],(.2,.4,.6,1,5)))
                self.mass_distribution[scope,r['variant'],name,label,r['contact_region'] or 'no_entry']+=1
            for stage,b1,b2 in strata(r):
                for name,m in results.items():
                    key=(scope,r['variant'],name,stage,b1,b2);g=self.summary[key];g['states']+=1
                    g['geometry_contacts']+=r['status']=='contact';g['impulse_contacts']+=m['collision_status']=='impulse_contact'
                    g['infinite_mass_contacts']+=m['infinite_effective_mass']
                    for k in METRICS:
                        if m[k] is not None:g[k]+=m[k];g[k+'_n']+=1
                    if m['ev_ms'] is not None:g['ev40']+=m['ev_ms']>=40;g['ev25']+=m['ev_ms']<25
                    if m['hla_deg'] is not None:g['foul']+=abs(m['hla_deg'])>=45
                    if m['vla_deg'] is not None:g['negative']+=m['vla_deg']<0
                    if direction(m)=='fair_direction':
                        g['fair_contacts']+=1;g['fair_ev']+=m['ev_ms'];g['fair_ev40']+=m['ev_ms']>=40;g['fair_ev25']+=m['ev_ms']<25
                    if name=='constant_0.6kg':continue
                    before=results['constant_0.6kg'];p=self.pairs[key];p['states']+=1
                    self.transitions[key+(r['status'],before['collision_status'],m['collision_status'],direction(before),direction(m))]+=1
                    if before['collision_status']!=m['collision_status'] or m['collision_status']!='impulse_contact':continue
                    p['paired_impulse_contacts']+=1
                    if direction(before)==direction(m)=='fair_direction':
                        p['paired_fair_contacts']+=1;p['fair_ev_before']+=before['ev_ms'];p['fair_ev_after']+=m['ev_ms']
                    for k in METRICS:
                        if before[k] is None or m[k] is None:continue
                        p[k+'_n']+=1;p[k+'_before']+=before[k];p[k+'_after']+=m[k]
                        delta=m[k]-before[k]
                        if k=='hla_deg':delta=(delta+180)%360-180
                        p[k+'_delta']+=delta;p[k+'_abs_delta']+=abs(delta)
    def write(self,output):
        rows=[]
        for key,g in sorted(self.summary.items()):
            r=dict(zip(KEYS,key),states=g['states'],geometry_contacts=g['geometry_contacts'],impulse_contacts=g['impulse_contacts'],
                   infinite_mass_contacts=g['infinite_mass_contacts'],finite_mass_contacts=g['effective_mass_kg_n'])
            for k in METRICS:r['avg_'+k]=g[k]/g[k+'_n'] if g[k+'_n'] else None
            fair=g['fair_contacts']
            r.update(fair_direction_contacts=fair,avg_fair_ev_ms=g['fair_ev']/fair if fair else None,
                fair_ev_ge_40_pct=100*g['fair_ev40']/fair if fair else None,
                fair_ev_lt_25_pct=100*g['fair_ev25']/fair if fair else None)
            r.update(defined_hla_contacts=g['hla_deg_n'],defined_vla_contacts=g['vla_deg_n'],
                ev_ge_40_pct=100*g['ev40']/g['ev_ms_n'] if g['ev_ms_n'] else None,
                ev_lt_25_pct=100*g['ev25']/g['ev_ms_n'] if g['ev_ms_n'] else None,
                foul_direction_pct=100*g['foul']/g['hla_deg_n'] if g['hla_deg_n'] else None,
                negative_vla_pct=100*g['negative']/g['vla_deg_n'] if g['vla_deg_n'] else None)
            rows.append(r)
        write_csv(output/'effective_mass_summary.csv',KEYS+['states','geometry_contacts','impulse_contacts','infinite_mass_contacts','finite_mass_contacts',
            'fair_direction_contacts','avg_fair_ev_ms','fair_ev_ge_40_pct','fair_ev_lt_25_pct','defined_hla_contacts','defined_vla_contacts','ev_ge_40_pct','ev_lt_25_pct','foul_direction_pct','negative_vla_pct']+['avg_'+k for k in METRICS],rows)
        rows=[]
        for key,g in sorted(self.pairs.items()):
            r=dict(zip(KEYS,key),states=g['states'],paired_impulse_contacts=g['paired_impulse_contacts'])
            fair=g['paired_fair_contacts'];r['paired_fair_contacts']=fair
            r['before_fair_ev_ms']=g['fair_ev_before']/fair if fair else None
            r['after_fair_ev_ms']=g['fair_ev_after']/fair if fair else None
            for k in METRICS:
                n=g[k+'_n'];r['pairs_'+k]=n
                for p,s in [('before_','_before'),('after_','_after'),('delta_','_delta'),('abs_delta_','_abs_delta')]:r[p+k]=g[k+s]/n if n else None
            rows.append(r)
        write_csv(output/'effective_mass_pairs.csv',KEYS+['states','paired_impulse_contacts','paired_fair_contacts','before_fair_ev_ms','after_fair_ev_ms']+[p+k for k in METRICS for p in ('pairs_','before_','after_','delta_','abs_delta_')],rows)
        dist_keys=['scope','variant','mass_model','mass_band_kg','contact_region']
        write_csv(output/'effective_mass_distribution.csv',dist_keys+['states'],
            [dict(zip(dist_keys,k),states=n) for k,n in sorted(self.mass_distribution.items())])
        keys=KEYS+['search_status','before_collision_status','after_collision_status','before_direction','after_direction']
        write_csv(output/'effective_mass_transitions.csv',keys+['states'],[dict(zip(keys,k),states=n) for k,n in sorted(self.transitions.items())])


def run(input_dir,output,gyrations=None,body_mass=None,cm_offset=None,*,preset=None):
    provenance=None
    if preset is not None:
        if preset!=PRESET:raise ValueError('unknown_bat_preset')
        if any(x is not None for x in (gyrations,body_mass,cm_offset)):raise ValueError('preset_cannot_be_combined_with_mass_overrides')
        provenance=measured_wood()
        body_mass=provenance['body_mass_kg'];cm_offset=provenance['cm_offset_from_reference_m']
        gyrations=(provenance['gyration_radius_m'],)
    else:
        gyrations=(.15,.25,.35) if gyrations is None else gyrations
        body_mass=.6 if body_mass is None else body_mass;cm_offset=0. if cm_offset is None else cm_offset
    if (not gyrations or len(set(gyrations))!=len(gyrations) or any(not math.isfinite(k) or k<=0 for k in gyrations)
        or not all(math.isfinite(x) for x in (body_mass,cm_offset)) or body_mass<=0):raise ValueError('invalid_mass_parameters')
    source=Path(input_dir);output=Path(output)
    paths={n:source/n for n in ('observation_update_contacts.csv','observation_update_metadata.json','observation_update_coverage.csv')}
    hashes={n:digest(p) for n,p in paths.items()}
    meta=json.loads(paths['observation_update_metadata.json'].read_text())
    if meta.get('version')!=1 or meta.get('restitution')!=.5 or meta.get('effective_mass_kg')!=.6:raise ValueError('unsupported_source_metadata')
    with paths['observation_update_coverage.csv'].open(newline='') as f:coverage=list(csv.DictReader(f))
    if len(coverage)!=1:raise ValueError('invalid_source_coverage')
    expected=int(coverage[0]['trial_rows']);conditions={'prior_only','exact_observation_control'}|{f'observed_sigma_{s:g}' for s in meta['sensor_sigma_m']}
    if expected!=int(coverage[0]['valid_histories'])*len(meta['levers_m'])*len(conditions):raise ValueError('invalid_source_count')
    configs=models(gyrations) if provenance is None else [('constant_0.6kg','constant',.25)]+[(f'{model}_{PRESET}',model,gyrations[0]) for model in ('free','pivot')]
    output.mkdir(parents=True,exist_ok=False);checks=defaultdict(float);counts=Counter();status=Counter();agg=Aggregate()
    with tempfile.TemporaryDirectory(prefix='saberbb_mass_') as temp:
        db=sqlite3.connect(str(Path(temp)/'keys.db'))
        try:
            db.execute('CREATE TABLE keys(game INTEGER, seq INTEGER, lever REAL, condition TEXT, PRIMARY KEY(game,seq,lever,condition))')
            with paths['observation_update_contacts.csv'].open(newline='') as f:
                reader=csv.DictReader(f);required=REQUIRED+['omega_'+a+'_rad_s' for a in 'xyz']
                if not set(required)<=set(reader.fieldnames or []) or len(reader.fieldnames)!=len(set(reader.fieldnames)):raise ValueError('missing_or_duplicate_csv_columns')
                for line,raw in enumerate(reader,2):
                    try:
                        r=audit(raw,checks)
                        if (r['forecast_condition'] not in conditions or r['lever_m'] not in meta['levers_m'] or r['half_length_m']!=meta['half_length_m']
                            or r['effective_radius_m']!=meta['radius_m']):raise ValueError('source_geometry_mismatch')
                        db.execute('INSERT INTO keys VALUES(?,?,?,?)',(r['game_id'],r['count_seq'],r['lever_m'],r['forecast_condition']))
                        results=evaluate(r,configs,body_mass,cm_offset,checks);agg.add(r,results)
                    except (ValueError,TypeError,KeyError,sqlite3.IntegrityError) as exc:raise ValueError(f'CSV line {line}: {exc}') from exc
                    counts['trial_rows']+=1;counts['geometry_contacts']+=r['status']=='contact'
                    for name,m in results.items():status[r['variant'],name,r['status'],m['collision_status']]+=1
            if counts['trial_rows']!=expected:raise ValueError('source_coverage_mismatch')
            groups=db.execute('SELECT COUNT(*) FROM keys GROUP BY game,seq,lever').fetchall()
            if any(n!=len(conditions) for (n,) in groups) or len(groups)!=int(coverage[0]['valid_histories'])*len(meta['levers_m']):raise ValueError('incomplete_source_keys')
        finally:db.close()
    agg.write(output)
    keys=['variant','mass_model','search_status','collision_status']
    write_csv(output/'effective_mass_status.csv',keys+['states'],[dict(zip(keys,k),states=n) for k,n in sorted(status.items())])
    write_csv(output/'effective_mass_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    result={k:counts[k] for k in ('trial_rows','geometry_contacts')};result.update(mass_models=len(configs),collision_trial_rows=counts['trial_rows']*len(configs))
    write_csv(output/'effective_mass_coverage.csv',list(result),[result])
    (output/'effective_mass_metadata.json').write_text(json.dumps(dict(version=1,input_sha256=hashes,input_metadata=meta,
        bat_preset=provenance,model_names=[name for name,_,_ in configs],
        body_mass_kg=body_mass,cm_offset_from_reference_m=cm_offset,gyration_radii_m=list(gyrations),
        free_transverse_inertias_kg_m2=[body_mass*k*k for k in gyrations],pivot_inertias_kg_m2={str(l):[body_mass*(k*k+(l+cm_offset)**2) for k in gyrations] for l in meta['levers_m']},
        assumptions=('published mass properties with explicit reference mapping; frozen contact geometry and velocities' if provenance else
                     'user-specified sensitivity hypotheses, NOT measured bat parameters; frozen contact geometry and velocities'),
        infinite_mass='inverse mass zero is an exact limit, no arbitrary mass cap',
        note='Free recoil permits center-of-mass motion during impact; prescribed pivot has no translational recoil. No vibrations/spin/tangential impulse.'),indent=2)+'\n')
    return result


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--input-dir',type=Path,required=True);p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--preset',choices=[PRESET],help='Published wood-bat mass properties; incompatible with manual mass/CM/gyration overrides')
    p.add_argument('--gyration-radii-m',type=float,nargs='+');p.add_argument('--body-mass-kg',type=float)
    p.add_argument('--cm-offset-m',type=float);a=p.parse_args()
    try:print(run(a.input_dir,a.output_dir,a.gyration_radii_m,a.body_mass_kg,a.cm_offset_m,preset=a.preset))
    except (OSError,ValueError,sqlite3.Error) as exc:p.exit(1,f'Diagnostic failed: {exc}\n')
