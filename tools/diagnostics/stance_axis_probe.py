#!/usr/bin/env python3
"""Read-only rotating-plan comparison: height-derived axis vs independent stance angle."""
import argparse
import copy
import json
import math
import sqlite3
from collections import Counter, defaultdict
from pathlib import Path

from contact_collision_probe import write_csv
from contact_state_probe import SOURCE, read_rows, validate_state
from execution_frame_probe import FrameMotion
from forecast_contact_geometry import signed_band
from observation_update_probe import validate_history, standard_noise, variants
from rotating_bat_probe import result_record, norm, sub
from swing_prediction_probe import validate_prediction, angle_for_position, plan_swing

METRICS=['ev_ms','hla_deg','vla_deg','normal_z','normal_speed_fraction','bat_point_speed_ms',
         'planned_axis_angle_deg','execution_point_shift_m','plan_anchor_change_m','reference_velocity_change_ms','incoming_z_ms','impulse_z_ms','outgoing_z_ms']
KEYS=['scope','forecast_condition','lever_m','axis_model','z_error_band']


def make_plan(prediction,forecast,lever,stance_angle=None):
    """Only forecast/configuration inputs: a stance angle never reads forecast height."""
    p=copy.deepcopy(prediction);p['forecast']=forecast
    p['planned_bat_angle_deg']=angle_for_position(forecast['position_world_m']) if stance_angle is None else stance_angle
    plan=plan_swing(p,lever,.16)
    plan['planned_axis_angle_deg']=p['planned_bat_angle_deg']
    return plan


def evaluate(row,state,plan,base=None):
    base=base or plan
    m=FrameMotion(state,plan['lever_m'],.16,plan['pivot_world_m'],plan['pivot_velocity_world_ms'],
                  plan['actual_axis_world'],plan['actual_omega_world_rad_s'])
    trial=copy.deepcopy(state);truth=state['input']
    trial['input']['timing_offset_sec']=plan['nominal_time_sec']-truth['reference_time_sec_since_release']
    r=result_record(row,trial,m,'stance_axis_control',max(-.06,-truth['reference_time_sec_since_release']),.06,1e-6,4000)
    r['linear_infinite_status']='not_applicable';r['linear_infinite_entry_offset_ms']=None
    r.update(planned_axis_angle_deg=plan['planned_axis_angle_deg'],
             plan_anchor_change_m=norm(sub(plan['actual_point_world_m'],base['actual_point_world_m'])),
             reference_velocity_change_ms=norm(sub(plan['actual_velocity_world_ms'],base['actual_velocity_world_ms'])),
             execution_point_shift_m=norm(sub(plan['actual_point_world_m'],plan['ideal_point_world_m'])),
             incoming_z_ms=None,impulse_z_ms=None,outgoing_z_ms=None)
    if r['collision_status']=='impulse_contact':
        r.update(incoming_z_ms=r['ball_velocity_z_ms'],impulse_z_ms=r['impulse_delta_ms']*r['normal_z'],
                 outgoing_z_ms=r['outgoing_velocity_z_ms'])
    return r


class Aggregate:
    def __init__(self):self.summary=defaultdict(Counter);self.pairs=defaultdict(Counter);self.transitions=Counter()
    def add(self,kind,condition,lever,band,results):
        scopes=['all_swings']+(['recorded_batting_only'] if kind!='UnrecordedBatting' else [])
        for scope in scopes:
            for zband in ('all',band):
                for model,r in results.items():
                    key=(scope,condition,lever,model,zband);g=self.summary[key]
                    g['states']+=1;g[r['status']]+=1;g['impulse_contacts']+=r['collision_status']=='impulse_contact'
                    g['non_approaching']+=r['collision_status']=='non_approaching';g['cap_contacts']+=r['contact_region']=='cap'
                    for k in METRICS:
                        if r[k] is not None:g[k]+=r[k];g[k+'_n']+=1
                    if r['hla_deg'] is not None:g['foul']+=abs(r['hla_deg'])>=45
                    if r['vla_deg'] is not None:g['negative_vla']+=r['vla_deg']<0
                    if r['ev_ms'] is not None:g['ev40']+=r['ev_ms']>=40
                before=results['height_dependent']
                for model,after in results.items():
                    if model=='height_dependent':continue
                    key=(scope,condition,lever,model,zband);g=self.pairs[key];g['states']+=1
                    self.transitions[key+(before['status'],after['status'],before['collision_status'],after['collision_status'])]+=1
                    if not before['collision_status']==after['collision_status']=='impulse_contact':continue
                    g['paired_impulse_contacts']+=1
                    for k in METRICS:
                        if before[k] is None or after[k] is None:continue
                        g[k+'_n']+=1;g[k+'_before']+=before[k];g[k+'_after']+=after[k]
                        delta=after[k]-before[k]
                        if k=='hla_deg':delta=(delta+180)%360-180
                        g[k+'_delta']+=delta;g[k+'_abs_delta']+=abs(delta)
                    if before['hla_deg'] is not None and after['hla_deg'] is not None:
                        g['foul_before']+=abs(before['hla_deg'])>=45;g['foul_after']+=abs(after['hla_deg'])>=45
    def write(self,output):
        rows=[]
        for key,g in sorted(self.summary.items()):
            r=dict(zip(KEYS,key),states=g['states'])
            for k in ('contact','no_contact','overlap_at_start','unresolved','impulse_contacts','non_approaching','cap_contacts'):r[k]=g[k]
            for k in METRICS:r['avg_'+k]=g[k]/g[k+'_n'] if g[k+'_n'] else None
            r.update(defined_hla_contacts=g['hla_deg_n'],defined_vla_contacts=g['vla_deg_n'],
                contact_pct=100*g['contact']/g['states'],
                foul_direction_pct=100*g['foul']/g['hla_deg_n'] if g['hla_deg_n'] else None,
                negative_vla_pct=100*g['negative_vla']/g['vla_deg_n'] if g['vla_deg_n'] else None,
                ev_ge_40_pct=100*g['ev40']/g['ev_ms_n'] if g['ev_ms_n'] else None)
            rows.append(r)
        write_csv(output/'stance_axis_summary.csv',KEYS+['states','contact','no_contact','overlap_at_start','unresolved',
            'impulse_contacts','non_approaching','cap_contacts','defined_hla_contacts','defined_vla_contacts',
            'contact_pct','foul_direction_pct','negative_vla_pct','ev_ge_40_pct']+['avg_'+k for k in METRICS],rows)
        rows=[]
        for key,g in sorted(self.pairs.items()):
            r=dict(zip(KEYS,key),states=g['states'],paired_impulse_contacts=g['paired_impulse_contacts'])
            for k in METRICS:
                n=g[k+'_n'];r['pairs_'+k]=n
                for prefix,suffix in [('before_','_before'),('after_','_after'),('delta_','_delta'),('abs_delta_','_abs_delta')]:
                    r[prefix+k]=g[k+suffix]/n if n else None
            h=g['hla_deg_n'];r.update(foul_before_pct=100*g['foul_before']/h if h else None,
                                     foul_after_pct=100*g['foul_after']/h if h else None)
            rows.append(r)
        write_csv(output/'stance_axis_pairs.csv',KEYS+['states','paired_impulse_contacts']+
            [prefix+k for k in METRICS for prefix in ('pairs_','before_','after_','delta_','abs_delta_')]+['foul_before_pct','foul_after_pct'],rows)
        keys=KEYS+['before_search_status','after_search_status','before_collision_status','after_collision_status']
        write_csv(output/'stance_axis_transitions.csv',keys+['states'],[dict(zip(keys,k),states=n) for k,n in sorted(self.transitions.items())])


def run(db_path,first,last,output,angles=(30.,),levers=(.6,.9,1.2),sigmas=(.002,.005,.01)):
    if first>last or not angles or len(set(angles))!=len(angles) or any(not math.isfinite(x) or not 0<=x<=80 for x in angles):raise ValueError('invalid_angles_or_range')
    if not levers or len(set(levers))!=len(levers) or any(not math.isfinite(x) or x<=.16 for x in levers):raise ValueError('invalid_levers')
    if not sigmas or len(set(sigmas))!=len(sigmas) or any(not math.isfinite(x) or x<0 for x in sigmas):raise ValueError('invalid_sigmas')
    output=Path(output);db=sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro',uri=True);db.row_factory=sqlite3.Row
    try:
        db.execute('PRAGMA query_only=ON');db.execute('BEGIN');output.mkdir(parents=True,exist_ok=False)
        counts=Counter();statuses=Counter();checks=defaultdict(float);aggregate=Aggregate()
        for row in read_rows(db,first,last):
            counts['swings']+=1;kind=row['adjusted_contact_type']
            try:
                if row['state_version'] is None:raise ValueError('missing_contact_state')
                if row['state_version']!=1 or row['state_source']!=SOURCE or row['invalid_reason'] is not None:raise ValueError('invalid_contact_state')
                state=json.loads(row['state_json']);validate_state(state)
                if state.get('swing_prediction') is None:raise ValueError('missing_prediction')
                p,_=validate_prediction(state['swing_prediction']);history=validate_history(p)
                if history[-1]['time_sec']>=state['input']['reference_time_sec_since_release']:raise ValueError('history_after_plate')
                forecasts=list(variants(p,history,standard_noise(row['game_id'],row['count_seq']),sigmas))
            except (ValueError,TypeError,KeyError,OverflowError) as exc:
                statuses[kind,str(exc)]+=1;continue
            counts['valid_histories']+=1;statuses[kind,'valid_history']+=1
            for condition,f,_,_,_ in forecasts:
                if condition=='exact_observation_control':
                    checks['exact_observation_forecast_position_m']=max(checks['exact_observation_forecast_position_m'],norm(sub(f['position_world_m'],state['input']['ball_position_world_m'])))
                    checks['exact_observation_forecast_time_sec']=max(checks['exact_observation_forecast_time_sec'],abs(f['time_sec']-state['input']['reference_time_sec_since_release']))
                zband=signed_band(f['position_world_m'][2]-state['input']['ball_position_world_m'][2])
                for lever in levers:
                    base=make_plan(p,f,lever);plans={'height_dependent':base}
                    for angle in angles:plans[f'stance_{angle:g}deg']=make_plan(p,f,lever,angle)
                    results={}
                    for model,plan in plans.items():
                        checks['shared_predicted_target_m']=max(checks['shared_predicted_target_m'],norm(sub(plan['forecast_position_world_m'],base['forecast_position_world_m'])))
                        checks['shared_nominal_time_sec']=max(checks['shared_nominal_time_sec'],abs(plan['nominal_time_sec']-base['nominal_time_sec']))
                        checks['shared_ideal_velocity_ms']=max(checks['shared_ideal_velocity_ms'],norm(sub(plan['ideal_velocity_world_ms'],base['ideal_velocity_world_ms'])))
                        r=evaluate(row,state,plan,base);results[model]=r;counts['trial_rows']+=1
                        for suffix in ('position_error_m','velocity_error_ms','axis_error'):
                            checks['anchor_'+suffix]=max(checks['anchor_'+suffix],r['anchor_'+suffix])
                        if r['outgoing_z_ms'] is not None:
                            checks['vertical_velocity_closure_ms']=max(checks['vertical_velocity_closure_ms'],abs(r['outgoing_z_ms']-r['incoming_z_ms']-r['impulse_z_ms']))
                    aggregate.add(kind,condition,lever,zband,results)
        aggregate.write(output)
        coverage=dict(first_game_id=first,last_game_id=last,swings=counts['swings'],valid_histories=counts['valid_histories'],
            trials_per_history=(2+len(sigmas))*(1+len(angles))*len(levers),trial_rows=counts['trial_rows'])
        write_csv(output/'stance_axis_coverage.csv',list(coverage),[coverage])
        write_csv(output/'stance_axis_status.csv',['adjusted_contact_type','status','states'],
            [dict(adjusted_contact_type=k[0],status=k[1],states=n) for k,n in sorted(statuses.items())])
        write_csv(output/'stance_axis_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
        (output/'stance_axis_metadata.json').write_text(json.dumps(dict(version=1,stance_angles_deg=list(angles),
            stance_source='explicit_diagnostic_config_independent_of_forecast_and_truth_height',
            default_30deg='existing_height_angle_rule_at_zone_center_NOT_calibrated_stance',levers_m=list(levers),sensor_sigma_m=list(sigmas),
            observation_model='same_observation_update_v1_common_standard_noise',motion='existing_rotating_FrameMotion',
            execution_angle='saved_error_shared_all_forecasts_and_axis_models',half_length_m=.16,radius_m=.07,
            half_window_sec=.06,contact_tolerance_m=1e-6,max_iterations=4000,restitution=.5,effective_mass_kg=.6,
            note='Axis-rule change rebuilds the whole consistent plan; pivot/omega/execution offset may change. No live outcome changes.'),indent=2)+'\n')
        return coverage
    finally:db.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--db',type=Path,required=True)
    p.add_argument('--first-game-id',type=int,required=True);p.add_argument('--last-game-id',type=int,required=True)
    p.add_argument('--output-dir',type=Path,required=True);p.add_argument('--stance-angles-deg',type=float,nargs='+',default=[30.])
    p.add_argument('--levers-m',type=float,nargs='+',default=[.6,.9,1.2]);p.add_argument('--sensor-sigma-m',type=float,nargs='+',default=[.002,.005,.01])
    a=p.parse_args()
    try:print(run(a.db,a.first_game_id,a.last_game_id,a.output_dir,a.stance_angles_deg,a.levers_m,a.sensor_sigma_m))
    except (OSError,ValueError,sqlite3.Error) as exc:p.exit(1,f'Diagnostic failed: {exc}\n')
