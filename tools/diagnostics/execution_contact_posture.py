#!/usr/bin/env python3
"""Replay saved execution entry times without contact search; audit vertical response."""
import argparse
import csv
import hashlib
import json
import math
import sqlite3
from collections import Counter, defaultdict
from itertools import zip_longest
from pathlib import Path

from bat_axis_origin_audit import check, tilt
from bat_pivot_orientation_probe import contact_at_time, MODEL_NAMES
from contact_collision_probe import BALL_MASS, angles, dot, write_csv
from contact_3d import band
from contact_state_probe import SOURCE, validate_state
from diagnostic_db import add_db_argument
from effective_mass_probe import direction
from finite_barrel_probe import configs, event
from forecast_contact_geometry import audit, vec, local_impact
from pitch_spin_source import PitchSpinSource
from rotating_bat_probe import add, sub, mul, norm
from statcast_distribution_compare import load_csv
from swing_execution import SwingPlan, ExecutionDeviation, PlanExecutionMotion, SwingExecution

METRICS = ('ev_ms','vla_deg','normal_z','normal_elevation_deg','axis_elevation_deg',
    'relative_z_ms','incoming_z_ms','bat_point_z_ms','bat_point_attack_deg',
    'normal_speed_fraction','local_signed_b_over_r','signed_axial_fraction',
    'normal_delta_z_ms','tangent_delta_z_ms','outgoing_z_ms','outgoing_horizontal_ms',
    'normal_only_vla_deg','tangential_vla_change_deg')
EV_EDGES = (0,10,20,25,30,35,40,45,50,55,60)
VLA_EDGES = (-90,-60,-30,0,10,20,30,40,50,60,90)
SUMMARY_KEYS = ['scenario','scope','model','cohort','stratum','band_1','band_2']


def digest(path):
    h=hashlib.sha256()
    with Path(path).open('rb') as f:
        for chunk in iter(lambda:f.read(1024*1024),b''):h.update(chunk)
    return h.hexdigest()


def same(expected,actual,checks,label):
    if isinstance(expected,dict):
        if not isinstance(actual,dict) or set(expected)-set(actual):raise ValueError('missing_saved_fields:'+label)
        for key,value in expected.items():same(value,actual[key],checks,label+'.'+key)
    elif isinstance(expected,(tuple,list)):
        if not isinstance(actual,(tuple,list)) or len(expected)!=len(actual):raise ValueError('invalid_saved_vector:'+label)
        for e,a in zip(expected,actual):same(e,a,checks,label)
    elif isinstance(expected,(int,float)):
        if not isinstance(actual,(int,float)) or not math.isfinite(actual):raise ValueError('nonfinite_saved_value:'+label)
        check(checks,label,abs(expected-actual))
    elif expected!=actual:raise ValueError('saved_state_mismatch:'+label)


def source_rows(folder,variant):
    with (folder/'observation_update_inputs.jsonl').open() as jf, (folder/'observation_update_contacts.csv').open(newline='') as cf:
        for raw,row in zip_longest(jf,csv.DictReader(cf)):
            if raw is None or row is None:raise ValueError('source_row_alignment_mismatch')
            if row['variant']!=variant:continue
            record=json.loads(raw)
            if (record['game_id'],record['count_seq'],f"{record['condition']}_r{record['plan']['lever_m']:g}") != (int(row['game_id']),int(row['count_seq']),row['variant']):
                raise ValueError('source_key_alignment_mismatch')
            if row['variant']==variant:yield record,row


def posture(row,result,checks):
    n=vec(row,'normal_');a=vec(row,'axis_at_entry_')
    vin=vec(row,'ball_velocity_');vbat=vec(row,'point_velocity_');relative=sub(vin,vbat)
    b=local_impact(a,n,relative,row['contact_region'])
    if b is not None:b*=1+row['min_evaluated_clearance_m']/row['effective_radius_m']
    values=dict(normal_z=n[2],normal_elevation_deg=math.degrees(math.asin(max(-1,min(1,n[2])))),
        axis_elevation_deg=math.degrees(math.asin(max(-1,min(1,a[2])))),
        relative_z_ms=relative[2],incoming_z_ms=vin[2],bat_point_z_ms=vbat[2],
        bat_point_attack_deg=angles(vbat)[2],
        normal_speed_fraction=max(0,min(1,-dot(relative,n)/norm(relative))) if norm(relative) else None,
        local_signed_b_over_r=b,signed_axial_fraction=row['closest_axial_at_entry_m']/row['half_length_m'])
    if result['collision_status']=='impulse_contact':
        out=result['outgoing_world_ms']
        normal=mul(result.get('normal_impulse_world_kg_ms',mul(n,result['normal_impulse_kg_ms'])),1/BALL_MASS)
        tangent=mul(result.get('tangent_impulse_world_kg_ms',(0.,0.,0.)),1/BALL_MASS)
        check(checks,'impulse_velocity_closure_ms',norm(sub(out,add(vin,add(normal,tangent)))),1e-7)
        ev,_,vla=angles(out)
        check(checks,'vector_ev_ms',abs(ev-result['ev_ms']),1e-7)
        check(checks,'vector_vla_deg',abs(vla-result['vla_deg']),1e-7)
        normal_vla=angles(add(vin,normal))[2]
        values.update(ev_ms=ev,vla_deg=vla,normal_delta_z_ms=normal[2],tangent_delta_z_ms=tangent[2],
            outgoing_z_ms=out[2],outgoing_horizontal_ms=math.hypot(out[0],out[1]),normal_only_vla_deg=normal_vla,
            tangential_vla_change_deg=vla-normal_vla if normal_vla is not None else None)
    return values


def strata(v):
    normal=band(v['normal_elevation_deg'],(-60,-30,-10,0,10,30,60))
    axis=band(v['axis_elevation_deg'],(-60,-30,0,30,60))
    yield 'overall','all','all'
    yield 'vla',band(v['vla_deg'],(-30,0,10,20,30,50,70)),'all'
    yield 'normal_relative_z',normal,band(v['relative_z_ms'],(-20,-5,0,5,20))
    yield 'axis_normal',axis,normal
    yield 'impact_axis',band(v['local_signed_b_over_r'],(-.95,-.75,-.25,0,.25,.75,.95)),axis


class Aggregate:
    def __init__(self):
        self.summary=defaultdict(Counter);self.selection=Counter();self.pairs=defaultdict(Counter)
        self.replay=defaultdict(Counter);self.hist=Counter();self.coverage=Counter()

    def trial(self,scenario,scopes,invalid):
        for scope in scopes:
            for model in MODEL_NAMES:
                g=self.replay[scenario,scope,model];g['trials']+=1
                g['execution_clock_invalid_trials']+=invalid
                g['evaluable_trials']+=not invalid

    def contact(self,scenario,scopes,row,results,checks):
        for model,m in results.items():
            v=posture(row,m,checks);success=m['collision_status']=='impulse_contact';fair=direction(m)=='fair_direction'
            for scope in scopes:
                g=self.replay[scenario,scope,model];g['geometry_contacts']+=1
                g['evaluable_geometry_contacts']+=1;g['successful_contacts']+=success;g['fair_contacts']+=fair
                g['normal_z']+=v['normal_z'];g['entry_tilt_deg']+=tilt(vec(row,'axis_at_entry_'))
                g['point_speed_ms']+=norm(vec(row,'point_velocity_'))
                self.selection[scenario,scope,model,band(v['normal_elevation_deg'],(-60,-30,-10,0,10,30,60)),
                    row['contact_region'],m['collision_status'],direction(m)]+=1
                if not success:continue
                for metric in ('ev_ms','vla_deg'):g[metric]+=m[metric]
                for cohort in ('all_successful','fair_direction'):
                    if cohort=='fair_direction' and not fair:continue
                    self.hist[scenario,scope,model,cohort,band(m['ev_ms'],EV_EDGES),band(m['vla_deg'],VLA_EDGES)]+=1
                    for stage,b1,b2 in strata(v):
                        a=self.summary[scenario,scope,model,cohort,stage,b1,b2];a['contacts']+=1
                        a['low_ev']+=m['ev_ms']<25;a['high_ev']+=m['ev_ms']>=45
                        a['downward']+=m['vla_deg']<0;a['high_vla']+=m['vla_deg']>=50
                        for metric in METRICS:
                            value=v.get(metric)
                            if value is not None:a[metric]+=value;a['n_'+metric]+=1
        before=results['normal_profile'];after=results['finite_I0.00015_et0.4']
        if before['collision_status']!= 'impulse_contact' or after['collision_status']!='impulse_contact':return
        for scope in scopes:
            for cohort in ('common_successful','common_fair'):
                if cohort=='common_fair' and not all(direction(m)=='fair_direction' for m in (before,after)):continue
                g=self.pairs[scenario,scope,cohort];g['contacts']+=1
                for side,m in [('normal',before),('finite',after)]:
                    g[side+'_vla_deg']+=m['vla_deg'];g[side+'_ev_ms']+=m['ev_ms']
                    g[side+'_downward']+=m['vla_deg']<0;g[side+'_high_vla']+=m['vla_deg']>=50
                g['up_to_down']+=before['vla_deg']>=0 and after['vla_deg']<0
                g['down_to_up']+=before['vla_deg']<0 and after['vla_deg']>=0

    def validate(self,folder,variant,checks):
        found=set()
        for r in load_csv(folder/'swing_execution_summary.csv',['scenario','scope','variant','model','stratum','band','orientation']):
            if (r['variant'],r['stratum'],r['band'],r['orientation'])!=(variant,'overall','all','execution'):continue
            key=(r['scenario'],r['scope'],r['model'])
            if key in found or key not in self.replay:raise ValueError('duplicate_or_unknown_summary_key')
            found.add(key);g=self.replay[key]
            for field in ('trials','execution_clock_invalid_trials','evaluable_trials','geometry_contacts',
                          'evaluable_geometry_contacts','successful_contacts','fair_contacts'):
                if int(r[field])!=g[field]:raise ValueError('replay_count_mismatch:'+field+':'+str(key))
            for field in ('ev_ms','vla_deg','normal_z','entry_tilt_deg','point_speed_ms'):
                n=g['successful_contacts'] if field in ('ev_ms','vla_deg') else g['geometry_contacts']
                if int(r['n_'+field])!=n:raise ValueError('replay_metric_count_mismatch:'+field)
                if n:check(checks,'replay_avg_'+field,abs(float(r['avg_'+field])-g[field]/n),1e-7)
        if found!=set(self.replay):raise ValueError('missing_replay_summary')
        expected=Counter()
        for r in load_csv(folder/'swing_execution_ev_vla.csv',['scenario','scope','variant','orientation','model','cohort','ev_band_ms','vla_band_deg','contacts','cohort_contacts','fraction']):
            if (r['variant'],r['orientation'])!=(variant,'execution'):continue
            key=tuple(r[k] for k in ('scenario','scope','model','cohort','ev_band_ms','vla_band_deg'))
            if key in expected:raise ValueError('duplicate_replay_histogram_cell')
            n=int(r['contacts']);cohort=r['cohort'];g=self.replay.get(key[:3])
            if g is None or cohort not in ('all_successful','fair_direction'):raise ValueError('unknown_replay_histogram_key')
            total=g['successful_contacts' if cohort=='all_successful' else 'fair_contacts']
            if n<=0 or total!=int(r['cohort_contacts']) or not math.isclose(float(r['fraction']),n/total,abs_tol=1e-12):
                raise ValueError('invalid_replay_histogram_denominator')
            expected[key]=n
        if expected!=self.hist:raise ValueError('replay_histogram_mismatch')

    def write(self,out):
        fields=SUMMARY_KEYS+['contacts','low_ev','high_ev','downward','high_vla']+[p+k for k in METRICS for p in ('n_','avg_')]
        rows=[]
        for key,g in sorted(self.summary.items()):
            r=dict(zip(SUMMARY_KEYS,key),**{k:g[k] for k in ('contacts','low_ev','high_ev','downward','high_vla')})
            for k in METRICS:r['n_'+k]=g['n_'+k];r['avg_'+k]=g[k]/g['n_'+k] if g['n_'+k] else None
            rows.append(r)
        write_csv(out/'execution_contact_posture_summary.csv',fields,rows)
        keys=['scenario','scope','model','normal_elevation_band_deg','contact_region','collision_status','direction']
        write_csv(out/'execution_contact_posture_selection.csv',keys+['contacts'],[dict(zip(keys,k),contacts=n) for k,n in sorted(self.selection.items())])
        rows=[]
        for key,g in sorted(self.pairs.items()):
            r=dict(zip(['scenario','scope','cohort'],key),contacts=g['contacts'],up_to_down=g['up_to_down'],down_to_up=g['down_to_up'])
            for side in ('normal','finite'):
                for metric in ('vla_deg','ev_ms','downward','high_vla'):r['avg_'+side+'_'+metric]=g[side+'_'+metric]/g['contacts']
            rows.append(r)
        fields=['scenario','scope','cohort','contacts','up_to_down','down_to_up']+['avg_'+s+'_'+m for s in ('normal','finite') for m in ('vla_deg','ev_ms','downward','high_vla')]
        write_csv(out/'execution_contact_posture_pairs.csv',fields,rows)
        write_csv(out/'execution_contact_posture_coverage.csv',['metric','trials'],[dict(metric=k,trials=n) for k,n in sorted(self.coverage.items())])


def run(input_dir,source_dir,db_path,output):
    folder=Path(input_dir);source_dir=Path(source_dir);output=Path(output)
    meta=json.loads((folder/'swing_execution_metadata.json').read_text())
    if meta['version']!=2:raise ValueError('requires_plan_execution_metadata_v2')
    variant=meta['variant'];scenarios=meta['scenarios']
    if not scenarios:raise ValueError('empty_execution_scenarios')
    output.mkdir(parents=True,exist_ok=False)
    checks=defaultdict(float);a=Aggregate();spin=None
    db=sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro',uri=True)
    try:
        db.execute('PRAGMA query_only=ON');db.execute('BEGIN')
        spin=PitchSpinSource(db_path,meta['spin']['gyro_sign'],output/'invalid_pitch_spin.csv')
        models=[m for m in configs((.00015,)) if m['name'] in MODEL_NAMES]
        with (folder/'swing_execution_states.jsonl').open() as sf:
            seen=set()
            for record,raw in source_rows(source_dir,variant):
                row=audit(raw,checks);key=(row['game_id'],row['count_seq'])
                if key in seen:raise ValueError('duplicate_source_pitch')
                seen.add(key);a.coverage['source_trials']+=1
                found=db.execute('SELECT state_version,state_source,invalid_reason,state_json FROM player_game_contact_state WHERE game_id=? AND count_seq=?',key).fetchall()
                if len(found)!=1 or found[0][:3]!=(1,SOURCE,None):raise ValueError('invalid_or_missing_contact_state')
                state=json.loads(found[0][3]);validate_state(state);truth=state['input'];saved_plan=record['plan']
                if truth['handedness_sign']!=(1 if saved_plan['ideal_axis_world'][0]>0 else -1):raise ValueError('db_handedness_mismatch')
                for i,k in ((0,'x'),(2,'z')):
                    check(checks,'db_forecast_'+k,abs(saved_plan['forecast_position_world_m'][i]-truth['ball_position_world_m'][i]-row['forecast_error_'+k+'_m']))
                check(checks,'db_clock_sec',abs(saved_plan['nominal_time_sec']-truth['reference_time_sec_since_release']-row['forecast_time_error_ms']/1000))
                if row['status']=='contact':check(checks,'db_ball_velocity_ms',norm(sub(vec(row,'ball_velocity_'),truth['ball_velocity_world_ms'])))
                incoming=spin.get(dict(game_id=key[0],count_seq=key[1],variant=variant,
                    **{'ball_velocity_'+k+'_ms':v for k,v in zip('xyz',truth['ball_velocity_world_ms'])}))
                available=max(o['time_sec'] for o in record['observations']) if record.get('observations') else state['swing_prediction']['input']['observation']['time_sec']
                if not math.isfinite(available) or not 0<=available<saved_plan['nominal_time_sec']:raise ValueError('invalid_plan_available_clock')
                scopes=['all_swings']+(['recorded_batting_only'] if row['adjusted_contact_type']!='UnrecordedBatting' else [])
                for name,c in scenarios.items():
                    line=sf.readline()
                    if not line:raise ValueError('missing_saved_execution_row')
                    saved=json.loads(line);identity=saved.get('execution',saved)
                    if (saved['scenario'],identity['game_id'],identity['count_seq'])!=(name,*key):raise ValueError('saved_execution_key_mismatch')
                    same(available,saved['plan_available_time_sec'],checks,'plan_available_time_sec')
                    invalid=None
                    try:
                        plan=SwingPlan.from_saved(saved_plan,c['planned_effort'],meta['duration_sec'],c['anchor_fraction'])
                        deviation=ExecutionDeviation(c['execution_ratio'],c['start_offset_sec'])
                        clock=SwingExecution(*key,saved_plan['nominal_time_sec'],c['planned_effort']*c['execution_ratio'],meta['duration_sec'],c['anchor_fraction'],c['start_offset_sec'])
                    except ValueError as exc:
                        if str(exc)!='execution_starts_before_release':raise
                        invalid=str(exc)
                    if invalid is None and min(plan.start_time_sec,clock.start_time_sec)<available-1e-10:invalid='execution_starts_before_plan'
                    if saved.get('invalid_reason')!=invalid:raise ValueError('saved_clock_status_mismatch')
                    a.trial(name,scopes,invalid is not None)
                    if invalid:
                        same(dict(planned_effort=c['planned_effort'],execution_ratio=c['execution_ratio'],duration_sec=meta['duration_sec'],anchor_fraction=c['anchor_fraction'],start_offset_sec=c['start_offset_sec'],nominal_time_sec=saved_plan['nominal_time_sec']),saved,checks,'invalid_state')
                        a.coverage[name+':clock_invalid']+=1;continue
                    same(plan.record(),saved['plan'],checks,'saved_plan')
                    same(clock.record(),saved['execution'],checks,'saved_execution')
                    same(dict(acceleration_ratio=deviation.acceleration_ratio,start_delay_sec=deviation.start_delay_sec),saved['execution_deviation'],checks,'saved_deviation')
                    for field,source_field in [('lever_m','lever_m'),('half_length_m','half_length_m'),('pivot_world_m','pivot_world_m'),
                        ('reference_point_world_m','actual_point_world_m'),('reference_axis_world','actual_axis_world'),
                        ('base_omega_world_rad_s','actual_omega_world_rad_s'),('base_pivot_velocity_world_ms','pivot_velocity_world_ms')]:
                        same(saved_plan[source_field],saved[field],checks,'saved_'+field)
                    entry=saved['entry_time_sec']
                    if entry is None:
                        if saved['entry_gain'] is not None or saved['instantaneous_entry_omega_world_rad_s'] is not None:raise ValueError('entry_metrics_without_saved_entry')
                        a.coverage[name+':no_saved_entry']+=1;continue
                    if not isinstance(entry,(int,float)) or not math.isfinite(entry):raise ValueError('invalid_saved_entry_time')
                    t=entry-truth['reference_time_sec_since_release'];delay=plan.nominal_time_sec-truth['reference_time_sec_since_release']
                    if not row['window_start_ms']-1e-7<=t*1000<=row['window_end_ms']+1e-7:raise ValueError('saved_entry_outside_window')
                    motion=PlanExecutionMotion(state,plan,deviation,*key)
                    check(checks,'effective_radius_m',abs(motion.radius-row['effective_radius_m']))
                    contact=contact_at_time(row,motion,delay,t)
                    check(checks,'saved_entry_clearance_m',abs(contact['min_evaluated_clearance_m']),2e-6)
                    same(clock.gain(motion.phase(t,delay)),saved['entry_gain'],checks,'saved_entry_gain')
                    same(motion.angular_velocity(t,delay),saved['instantaneous_entry_omega_world_rad_s'],checks,'saved_entry_omega')
                    results,reason=event(contact,checks,models,incoming,None,include_ground=False)
                    a.coverage[name+':replayed_contacts']+=1;a.coverage[name+':'+reason]+=1
                    a.contact(name,scopes,contact,results,checks)
            if sf.readline():raise ValueError('extra_saved_execution_row')
        if len(seen)!=meta['source_coverage']['selected_rows']:raise ValueError('source_trial_count_mismatch')
        spin_meta=spin.metadata()
        if spin_meta['used_scalar_rows_sha256']!=meta['spin']['used_scalar_rows_sha256']:raise ValueError('source_pitch_spin_hash_mismatch')
        a.validate(folder,variant,checks)
        a.coverage['new_searches']=0;a.write(output)
        write_csv(output/'execution_contact_posture_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
        inputs=[folder/f for f in ('swing_execution_metadata.json','swing_execution_states.jsonl','swing_execution_summary.csv','swing_execution_ev_vla.csv')]+[source_dir/f for f in ('observation_update_inputs.jsonl','observation_update_contacts.csv')]
        metadata=dict(version=1,status='diagnostic_replay_not_calibrated',variant=variant,models=list(MODEL_NAMES),source_metadata=meta,
            spin=spin_meta,new_searches=0,source_trials=len(seen),inputs={str(p):digest(p) for p in inputs},
            replay_validation='all execution overall counts/EV/VLA/normal/tilt/speed means and EV/VLA histogram cells matched',
            vertical_identity='v_out = v_in + J_normal / ball_mass + J_tangent / ball_mass',
            limitations=['Saved entry times are trusted events, not independently searched or proven first contacts.',
                'No saved entry includes misses, overlaps, or unresolved searches; these are not relabeled as misses.',
                'Clock-invalid trials remain unevaluated. Geometry selection table retains cap/curve/collision exclusions.',
                'normal_only_vla removes the tangential impulse from this response, holding its normal impulse fixed; not a separate frictionless solution.',
                'all_successful and fair_direction are separately conditioned and scopes/strata/models overlap.',
                'Positive normal_z means bat-to-ball upward normal, not guaranteed upward outgoing velocity.',
                'Signed local b/R is an instantaneous cylinder-plane diagnostic, not the global rotating-path minimum.',
                'No Statcast fitting, production parameter changes, game/RNG/DB writes, or new contact searches.'])
        (output/'execution_contact_posture_metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
        return dict(source_trials=len(seen),replayed_contacts=sum(v for k,v in a.coverage.items() if k.endswith(':replayed_contacts')),new_searches=0)
    finally:
        if spin is not None:spin.close()
        db.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,required=True,help='Existing execution ratio sweep output')
    p.add_argument('--source-dir',type=Path,required=True,help='Original observation-update output')
    add_db_argument(p);p.add_argument('--output-dir',type=Path,required=True)
    a=p.parse_args()
    try:print(json.dumps(run(a.input_dir,a.source_dir,a.db,a.output_dir),indent=2))
    except (OSError,ValueError,KeyError,TypeError,sqlite3.Error) as exc:p.exit(1,f'Contact posture replay failed: {exc}\n')
