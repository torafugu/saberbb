#!/usr/bin/env python3
"""Frozen-contact collision control: local bat velocity vs nominal reference-point velocity."""
import argparse
import csv
import hashlib
import json
import math
import sqlite3
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

from contact_3d import ContactGeometry, solve_contact, band
from contact_collision_probe import cross, dot, write_csv
from forecast_contact_geometry import REQUIRED, audit, number, vec, signed_band
from rotating_bat_probe import add, sub, mul, norm, rotate
from swing_prediction_probe import PREDICTION_SOURCE, vector

MODES=('actual_point_velocity','planned_reference_velocity')
METRICS=['ev_ms','hla_deg','vla_deg','normal_speed_fraction','relative_normal_ms',
         'bat_point_speed_ms','bat_velocity_x_ms','bat_velocity_y_ms','bat_velocity_z_ms',
         'incoming_z_ms','impulse_z_ms','outgoing_z_ms']
KEYS=['scope','variant','stage','band_1','band_2']


def check(checks,name,value,tolerance=1e-7):
    if not math.isfinite(value):raise ValueError('non_finite_check:'+name)
    checks[name]=max(checks[name],value)
    if value>tolerance:raise ValueError('reconstruction_mismatch:'+name)


def validate_plan(record,checks):
    p=record['plan']
    if p['plan_version']!=1 or p['source']!=PREDICTION_SOURCE:raise ValueError('unsupported_plan')
    for field in ('lever_m','half_length_m','nominal_time_sec'):
        if not math.isfinite(p[field]) or p[field]<=0:raise ValueError('invalid_plan_scalar')
    if p['lever_m']<=p['half_length_m']:raise ValueError('invalid_plan_shape')
    for field in ('actual_axis_world','actual_velocity_world_ms','actual_omega_world_rad_s',
                  'pivot_velocity_world_ms','pivot_world_m','actual_point_world_m'):
        if not vector(p[field]):raise ValueError('invalid_plan_vector:'+field)
    a=p['actual_axis_world'];w=p['actual_omega_world_rad_s'];vp=p['pivot_velocity_world_ms'];lever=p['lever_m']
    check(checks,'plan_axis_unit',abs(dot(a,a)-1),1e-8)
    check(checks,'plan_reference_position_m',norm(sub(add(p['pivot_world_m'],mul(a,lever)),p['actual_point_world_m'])))
    check(checks,'plan_reference_velocity_ms',norm(sub(add(vp,cross(w,mul(a,lever))),p['actual_velocity_world_ms'])))
    return p


def compare(r,p,checks):
    """Geometry/time/incoming ball fixed. Only the velocity passed to solve_contact changes."""
    if r['status']!='contact':
        return {mode:dict(collision_status='not_evaluated:'+r['status'],**{k:None for k in METRICS}) for mode in MODES}
    axis=vec(r,'axis_at_entry_');normal=vec(r,'normal_');vin=vec(r,'ball_velocity_');local=vec(r,'point_velocity_')
    phase=r['entry_since_release_sec']-p['nominal_time_sec']
    expected_axis=rotate(p['actual_axis_world'],p['actual_omega_world_rad_s'],phase)
    expected_velocity=add(p['pivot_velocity_world_ms'],cross(p['actual_omega_world_rad_s'],
        mul(expected_axis,p['lever_m']+r['closest_axial_at_entry_m'])))
    check(checks,'entry_axis_from_plan',norm(sub(axis,expected_axis)))
    check(checks,'entry_velocity_from_plan_ms',norm(sub(local,expected_velocity)))
    plate_clock=r['entry_since_release_sec']-number(r,'entry_time_offset_ms')/1000
    check(checks,'nominal_clock_sec',abs(p['nominal_time_sec']-plate_clock-number(r,'timing_offset_ms')/1000),1e-10)
    geometry=ContactGeometry(r['entry_since_release_sec'],axis,normal,r['axial_at_entry_m'],
                             r['closest_axial_at_entry_m'],r['half_length_m'],r['contact_region'])
    result={}
    for mode,velocity in zip(MODES,(local,p['actual_velocity_world_ms'])):
        m=solve_contact(geometry,vin,velocity)
        m.update(bat_velocity_x_ms=velocity[0],bat_velocity_y_ms=velocity[1],bat_velocity_z_ms=velocity[2],
                 incoming_z_ms=None,impulse_z_ms=None,outgoing_z_ms=None)
        if m['outgoing_world_ms'] is not None:
            m.update(incoming_z_ms=vin[2],impulse_z_ms=m['impulse_delta_ms']*normal[2],outgoing_z_ms=m['outgoing_world_ms'][2])
            check(checks,'vertical_velocity_closure_ms',abs(m['outgoing_z_ms']-vin[2]-m['impulse_z_ms']))
        result[mode]=m
    if result[MODES[0]]['collision_status']!=r['collision_status']:raise ValueError('baseline_collision_mismatch')
    return result


def strata(r):
    yield 'overall','all','all'
    yield 'forecast_z',signed_band(r['forecast_error_z_m']),'all'
    if r['status']=='contact':
        b=band(r['local_abs_b_over_r'],(.25,.5,.75,.95)) if r['local_abs_b_over_r'] is not None else 'cap_or_undefined'
        yield 'baseline_impact',b,band(abs(r['signed_axial_fraction']),(.25,.5,.75))
        yield 'baseline_normal',band(r['normal_z'],(-.5,-.1,0,.1,.5)),band(r['normal_speed_fraction'],(.5,.8,.95))


class Accumulator:
    def __init__(self):self.summaries=defaultdict(Counter);self.pairs=defaultdict(Counter);self.transitions=Counter()
    def add(self,r,results):
        for scope in ('all_swings','recorded_batting_only'):
            if scope=='recorded_batting_only' and r['adjusted_contact_type']=='UnrecordedBatting':continue
            for stage,b1,b2 in strata(r):
                key=(scope,r['variant'],stage,b1,b2)
                before,after=(results[m] for m in MODES)
                self.transitions[key+(r['status'],before['collision_status'],after['collision_status'])]+=1
                pair=self.pairs[key];pair['states']+=1
                for mode,m in results.items():
                    g=self.summaries[key+(mode,)];g['states']+=1
                    g['geometry_contacts']+=r['status']=='contact'
                    g['impulse_contacts']+=m['collision_status']=='impulse_contact'
                    g['non_approaching']+=m['collision_status']=='non_approaching'
                    for k in METRICS:
                        if m[k] is not None:g[k]+=m[k];g[k+'_n']+=1
                    if m['hla_deg'] is not None:g['foul']+=abs(m['hla_deg'])>=45
                    if m['vla_deg'] is not None:g['negative']+=m['vla_deg']<0
                    if m['ev_ms'] is not None:g['ev40']+=m['ev_ms']>=40
                if not before['collision_status']==after['collision_status']=='impulse_contact':continue
                pair['paired_impulse_contacts']+=1
                for k in METRICS:
                    if before[k] is None or after[k] is None:continue
                    pair[k+'_n']+=1;pair[k+'_before']+=before[k];pair[k+'_after']+=after[k]
                    delta=after[k]-before[k]
                    if k=='hla_deg':delta=(delta+180)%360-180
                    pair[k+'_delta']+=delta;pair[k+'_abs_delta']+=abs(delta)
                if before['hla_deg'] is not None and after['hla_deg'] is not None:
                    pair['foul_before']+=abs(before['hla_deg'])>=45;pair['foul_after']+=abs(after['hla_deg'])>=45
    def write(self,output):
        rows=[]
        for key,g in sorted(self.summaries.items()):
            r=dict(zip(KEYS+['velocity_mode'],key),states=g['states'],geometry_contacts=g['geometry_contacts'],
                   impulse_contacts=g['impulse_contacts'],non_approaching=g['non_approaching'])
            for k in METRICS:r['avg_'+k]=g[k]/g[k+'_n'] if g[k+'_n'] else None
            r.update(defined_hla_contacts=g['hla_deg_n'],defined_vla_contacts=g['vla_deg_n'],
                foul_direction_pct=100*g['foul']/g['hla_deg_n'] if g['hla_deg_n'] else None,
                negative_vla_pct=100*g['negative']/g['vla_deg_n'] if g['vla_deg_n'] else None,
                ev_ge_40_pct=100*g['ev40']/g['ev_ms_n'] if g['ev_ms_n'] else None)
            rows.append(r)
        write_csv(output/'contact_speed_summary.csv',KEYS+['velocity_mode','states','geometry_contacts','impulse_contacts',
            'non_approaching','defined_hla_contacts','defined_vla_contacts','foul_direction_pct','negative_vla_pct','ev_ge_40_pct']+
            ['avg_'+k for k in METRICS],rows)
        rows=[]
        for key,g in sorted(self.pairs.items()):
            n=g['paired_impulse_contacts'];r=dict(zip(KEYS,key),states=g['states'],paired_impulse_contacts=n)
            for k in METRICS:
                count=g[k+'_n'];r['pairs_'+k]=count
                for prefix,suffix in [('before_','_before'),('after_','_after'),('delta_','_delta'),('abs_delta_','_abs_delta')]:
                    r[prefix+k]=g[k+suffix]/count if count else None
            hla_n=g['hla_deg_n']
            r.update(foul_before_pct=100*g['foul_before']/hla_n if hla_n else None,foul_after_pct=100*g['foul_after']/hla_n if hla_n else None)
            rows.append(r)
        write_csv(output/'contact_speed_pairs.csv',KEYS+['states','paired_impulse_contacts']+
            [prefix+k for k in METRICS for prefix in ('pairs_','before_','after_','delta_','abs_delta_')]+['foul_before_pct','foul_after_pct'],rows)
        keys=KEYS+['search_status','before_collision_status','after_collision_status']
        write_csv(output/'contact_speed_transitions.csv',keys+['states'],
            [dict(zip(keys,key),states=n) for key,n in sorted(self.transitions.items())])


def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda:f.read(1024*1024),b''):h.update(chunk)
    return h.hexdigest()


def run(input_dir,output):
    source=Path(input_dir);output=Path(output)
    paths={name:source/name for name in ['observation_update_contacts.csv','observation_update_inputs.jsonl',
        'observation_update_metadata.json','observation_update_coverage.csv']}
    hashes={name:digest(path) for name,path in paths.items()}
    metadata=json.loads(paths['observation_update_metadata.json'].read_text())
    if metadata.get('version')!=1 or metadata.get('restitution')!=.5 or metadata.get('effective_mass_kg')!=.6:
        raise ValueError('unsupported_observation_metadata')
    with paths['observation_update_coverage.csv'].open(newline='') as f:coverage=list(csv.DictReader(f))
    if len(coverage)!=1:raise ValueError('invalid_source_coverage')
    expected=int(coverage[0]['trial_rows']);valid_histories=int(coverage[0]['valid_histories'])
    conditions={'prior_only','exact_observation_control'}|{f'observed_sigma_{s:g}' for s in metadata['sensor_sigma_m']}
    if expected!=valid_histories*len(metadata['levers_m'])*len(conditions):raise ValueError('invalid_expected_rows')
    output.mkdir(parents=True,exist_ok=False)
    checks=defaultdict(float);counts=Counter();statuses=Counter();aggregate=Accumulator()
    with tempfile.TemporaryDirectory(prefix='saberbb_contact_speed_') as temp:
        db=sqlite3.connect(str(Path(temp)/'plans.db'))
        try:
            db.execute('CREATE TABLE plans(game INTEGER,seq INTEGER,lever REAL,condition TEXT,payload TEXT,used INTEGER DEFAULT 0, PRIMARY KEY(game,seq,lever,condition))')
            with paths['observation_update_inputs.jsonl'].open() as f:
                for line,text in enumerate(f,1):
                    try:
                        record=json.loads(text);p=validate_plan(record,checks)
                        if record['condition'] not in conditions or p['lever_m'] not in metadata['levers_m'] or p['half_length_m']!=metadata['half_length_m']:
                            raise ValueError('plan_metadata_mismatch')
                        game,seq=record['game_id'],record['count_seq']
                        if type(game) is not int or type(seq) is not int or game<0 or seq<0:raise ValueError('invalid_plan_key')
                        db.execute('INSERT INTO plans(game,seq,lever,condition,payload) VALUES(?,?,?,?,?)',
                            (game,seq,p['lever_m'],record['condition'],json.dumps({k:p[k] for k in (
                                'lever_m','half_length_m','nominal_time_sec','actual_axis_world','actual_omega_world_rad_s',
                                'pivot_velocity_world_ms','actual_velocity_world_ms')})))
                    except (ValueError,TypeError,KeyError,sqlite3.IntegrityError) as exc:
                        raise ValueError(f'plan JSONL line {line}: {exc}') from exc
                    counts['plan_rows']+=1
            if counts['plan_rows']!=expected:raise ValueError('plan_coverage_mismatch')
            db.commit()
            with paths['observation_update_contacts.csv'].open(newline='') as f:
                reader=csv.DictReader(f);required=REQUIRED+['timing_offset_ms']
                if not set(required)<=set(reader.fieldnames or []) or len(reader.fieldnames)!=len(set(reader.fieldnames)):
                    raise ValueError('missing_or_duplicate_csv_columns')
                for line,raw in enumerate(reader,2):
                    try:
                        r=audit(raw,checks);key=(r['game_id'],r['count_seq'],r['lever_m'],r['forecast_condition'])
                        item=db.execute('SELECT payload,used FROM plans WHERE game=? AND seq=? AND lever=? AND condition=?',key).fetchone()
                        if item is None:raise ValueError('missing_plan_key')
                        if item[1]:raise ValueError('duplicate_contact_key')
                        p=json.loads(item[0])
                        if (r['half_length_m']!=p['half_length_m'] or r['effective_radius_m']!=metadata['radius_m'] or
                            r['window_end_ms']!=metadata['half_window_sec']*1000):raise ValueError('contact_metadata_mismatch')
                        results=compare(r,p,checks);aggregate.add(r,results)
                        db.execute('UPDATE plans SET used=1 WHERE game=? AND seq=? AND lever=? AND condition=?',key)
                    except (ValueError,TypeError,KeyError) as exc:raise ValueError(f'contact CSV line {line}: {exc}') from exc
                    counts['trial_rows']+=1;counts['geometry_contacts']+=r['status']=='contact'
                    for mode,m in results.items():statuses[r['variant'],r['status'],mode,m['collision_status']]+=1
            if counts['trial_rows']!=expected:raise ValueError('contact_coverage_mismatch')
            if db.execute('SELECT COUNT(*) FROM plans WHERE used=0').fetchone()[0]:raise ValueError('unused_plan_keys')
            keys=db.execute('SELECT game,seq,lever,COUNT(*) FROM plans GROUP BY game,seq,lever').fetchall()
            if any(n!=len(conditions) for _,_,_,n in keys):raise ValueError('incomplete_conditions')
            if len(keys)!=valid_histories*len(metadata['levers_m']):raise ValueError('ball_lever_coverage_mismatch')
        finally:db.close()
    aggregate.write(output)
    keys=['variant','search_status','velocity_mode','collision_status']
    write_csv(output/'contact_speed_status.csv',keys+['states'],[dict(zip(keys,k),states=n) for k,n in sorted(statuses.items())])
    write_csv(output/'contact_speed_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    result={key:counts[key] for key in ('plan_rows','trial_rows','geometry_contacts')}
    result.update(velocity_modes=2,collision_trial_rows=2*counts['trial_rows'])
    write_csv(output/'contact_speed_coverage.csv',list(result),[result])
    (output/'contact_speed_metadata.json').write_text(json.dumps(dict(version=1,input_sha256=hashes,input_metadata=metadata,
        reference='plan.actual_velocity_world_ms_at_nominal_time_center_of_contact_segment',
        fixed='entry_geometry_time_normal_and_incoming_velocity',
        note='Frozen-event collision counterfactual; no new contact search. Baseline strata fixed for both modes.'),indent=2)+'\n')
    return result


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--input-dir',type=Path,required=True)
    p.add_argument('--output-dir',type=Path,required=True);a=p.parse_args()
    try:print(run(a.input_dir,a.output_dir))
    except (OSError,ValueError,sqlite3.Error) as exc:p.exit(1,f'Diagnostic failed: {exc}\n')
