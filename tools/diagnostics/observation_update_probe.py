#!/usr/bin/env python3
"""Read-only, causal three-position forecast experiment; sensor assumptions are uncalibrated."""
import argparse
from diagnostic_db import add_db_argument
import copy
import csv
import hashlib
import json
import math
import random
import sqlite3
from collections import Counter
from pathlib import Path

from contact_3d import QualityAccumulator
from contact_collision_probe import write_csv
from contact_state_probe import SOURCE, read_rows, validate_state
from execution_frame_probe import FrameMotion, Paired, SummaryAccumulator
from rotating_bat_probe import FIELDS, SUMMARY_FIELDS, result_record
from swing_prediction_probe import validate_prediction, forecast, vector, angle_for_position, plan_swing

EXTRA=['forecast_condition','sensor_sigma_m','eye_for_sensor','forecast_error_x_m',
       'forecast_error_z_m','forecast_time_error_ms']
ERROR_FIELDS=['game_id','count_seq','adjusted_contact_type','forecast_condition','sensor_sigma_m',
              'eye_for_sensor','forecast_time_sec','forecast_error_x_m','forecast_error_z_m','forecast_time_error_ms']


def validate_history(p):
    history=p.get('observation_history')
    if history is None:raise ValueError('missing_observation_history')
    if len(history)!=3:raise ValueError('invalid_history_count')
    times=[o['time_sec'] for o in history]
    if not all(isinstance(t,(int,float)) and math.isfinite(t) and t>0 for t in times):
        raise ValueError('invalid_history_clock')
    if not times[0]<times[1]<times[2]:raise ValueError('unordered_history')
    for o,ratio in zip(history,(.2,.4,.6)):
        if abs(o['time_sec']-ratio*p['prior_flight_time_sec'])>1e-10 or not vector(o['position_world_m']):
            raise ValueError('history_clock_or_position')
        if o['position_world_m'][1]<=0:raise ValueError('history_not_before_plate')
    if abs(times[2]-p['observation']['time_sec'])>1e-10 or any(
        abs(a-b)>1e-10 for a,b in zip(history[-1]['position_world_m'],p['observation']['position_world_m'])):
        raise ValueError('history_commit_mismatch')
    return history


def sensor_scale(eye):
    if not math.isfinite(eye):raise ValueError('invalid_eye')
    # Explicit hypothesis, NOT a fitted or physiological measurement.
    return math.exp(-.5*max(-4.,min(4.,eye)))


def standard_noise(game_id,count_seq):
    seed=hashlib.sha256(f'observation_sensor_v1:{game_id}:{count_seq}'.encode()).digest()
    rng=random.Random(int.from_bytes(seed,'big'))
    return [[rng.gauss(0.,1.) for _ in range(3)] for _ in range(3)]


def observe(history,sigma,eye,noise):
    if not math.isfinite(sigma) or sigma<0:raise ValueError('invalid_sigma')
    scale=sigma*sensor_scale(eye)
    return [dict(time_sec=o['time_sec'],position_world_m=[x+scale*z for x,z in zip(o['position_world_m'],n)])
            for o,n in zip(history,noise)]


def estimate(history):
    """Quadratic interpolation using ONLY the three supplied, past position observations."""
    t0,t1,t2=[o['time_sec'] for o in history]
    if not 0<t0<t1<t2:raise ValueError('invalid_estimation_clock')
    p0,p1,p2=[o['position_world_m'] for o in history]
    if not all(vector(p) for p in (p0,p1,p2)):raise ValueError('invalid_estimation_position')
    a=[2*((z-y)/(t2-t1)-(y-x)/(t1-t0))/(t2-t0) for x,y,z in zip(p0,p1,p2)]
    v=[(z-y)/(t2-t1)+.5*b*(t2-t1) for y,z,b in zip(p1,p2,a)]
    # Simulator uses constant forward velocity. Project Y acceleration to zero;
    # estimate forward velocity over the whole observed interval to reduce noise.
    a[1]=0.;v[1]=(p2[1]-p0[1])/(t2-t0)
    return forecast(history[-1],v,a),v,a


def variants(p,history,noise,sigmas):
    yield 'prior_only',p['forecast'],None,None,None
    for sigma in sigmas:
        observations=observe(history,sigma,p['batting_eye'],noise)
        f,v,a=estimate(observations)
        yield f'observed_sigma_{sigma:g}',f,sigma,observations,dict(velocity=v,acceleration=a)
    f,v,a=estimate(history)
    yield 'exact_observation_control',f,0.,history,dict(velocity=v,acceleration=a)


def error_row(row,truth,f,condition,sigma,eye):
    return dict(game_id=row['game_id'],count_seq=row['count_seq'],adjusted_contact_type=row['adjusted_contact_type'],
        forecast_condition=condition,sensor_sigma_m=sigma,eye_for_sensor=eye,forecast_time_sec=f['time_sec'],
        forecast_error_x_m=f['position_world_m'][0]-truth['ball_position_world_m'][0],
        forecast_error_z_m=f['position_world_m'][2]-truth['ball_position_world_m'][2],
        forecast_time_error_ms=1000*(f['time_sec']-truth['reference_time_sec_since_release']))


def error_summary(rows):
    groups={}
    for row in rows:
        eye=row['eye_for_sensor']
        band='eye_lt_-1' if eye < -1 else 'eye_-1_to_1' if eye <= 1 else 'eye_gt_1'
        for b in ('all',band):
            groups.setdefault((row['forecast_condition'],row['sensor_sigma_m'],b),[]).append(row)
    output=[]
    for (condition,sigma,band),group in sorted(groups.items(),key=lambda item:str(item[0])):
        result=dict(forecast_condition=condition,sensor_sigma_m=sigma,eye_band=band,balls=len(group))
        for axis,field in [('x_m','forecast_error_x_m'),('z_m','forecast_error_z_m'),('time_ms','forecast_time_error_ms')]:
            values=[r[field] for r in group];ordered=sorted(abs(x) for x in values)
            result['mean_signed_'+axis]=sum(values)/len(values)
            result['mean_abs_'+axis]=sum(ordered)/len(ordered)
            for q in (.5,.9,.99):
                i=(len(ordered)-1)*q;lo=int(i);hi=min(lo+1,len(ordered)-1)
                result[f'p{int(q*100)}_abs_'+axis]=ordered[lo]+(ordered[hi]-ordered[lo])*(i-lo)
        output.append(result)
    return output

ERROR_SUMMARY_FIELDS=['forecast_condition','sensor_sigma_m','eye_band','balls']+[
    metric+'_'+axis for axis in ('x_m','z_m','time_ms')
    for metric in ('mean_signed','mean_abs','p50_abs','p90_abs','p99_abs')]


def run(db_path,first,last,output,sigmas=(.002,.005,.01),levers=(.6,.9,1.2)):
    if first>last or not sigmas or len(set(sigmas))!=len(sigmas) or any(not math.isfinite(s) or s<0 for s in sigmas):
        raise ValueError('invalid_parameters')
    if not levers or len(set(levers))!=len(levers) or any(not math.isfinite(l) or l<=.16 for l in levers):
        raise ValueError('invalid_levers')
    db=sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro',uri=True);db.row_factory=sqlite3.Row
    output=Path(output)
    try:
        db.execute('PRAGMA query_only=ON');db.execute('BEGIN');output.mkdir(parents=True,exist_ok=False)
        counts=Counter();statuses=Counter();summary=SummaryAccumulator();paired=Paired();quality=QualityAccumulator()
        errors=[];sensitivity=[]
        with (output/'observation_update_contacts.csv').open('x',newline='') as cf, (output/'observation_update_inputs.jsonl').open('x') as pf:
            writer=csv.DictWriter(cf,fieldnames=FIELDS+EXTRA);writer.writeheader()
            for row in read_rows(db,first,last):
                counts['swings']+=1;kind=row['adjusted_contact_type']
                try:
                    if row['state_version'] is None:raise ValueError('missing_contact_state')
                    if row['state_version']!=1 or row['state_source']!=SOURCE or row['invalid_reason'] is not None:
                        raise ValueError('invalid_contact_state')
                    state=json.loads(row['state_json']);validate_state(state)
                    if state.get('swing_prediction') is None:raise ValueError('missing_prediction')
                    p,_=validate_prediction(state['swing_prediction']);history=validate_history(p)
                    truth=state['input']
                    if history[-1]['time_sec']>=truth['reference_time_sec_since_release']:raise ValueError('history_after_plate')
                    noise=standard_noise(row['game_id'],row['count_seq'])
                    trials=list(variants(p,history,noise,sigmas))
                except (ValueError,TypeError,KeyError,OverflowError) as exc:
                    statuses[kind,str(exc)]+=1;continue
                counts['valid_histories']+=1;statuses[kind,'valid_history']+=1
                # Paired ability counterfactuals: same ball/noise, eye is the ONLY changed input.
                for sigma in sigmas:
                    for eye in (-2.,0.,2.):
                        f,_,_=estimate(observe(history,sigma,eye,noise))
                        sensitivity.append(error_row(row,truth,f,'eye_counterfactual',sigma,eye))
                for condition,f,sigma,observations,estimates in trials:
                    errors.append(error_row(row,truth,f,condition,sigma,p['batting_eye']))
                for lever in levers:
                    results={}
                    for condition,f,sigma,observations,estimates in trials:
                        candidate=copy.deepcopy(p);candidate['forecast']=f
                        candidate['planned_bat_angle_deg']=angle_for_position(f['position_world_m'])
                        # Keep execution-angle error identical to the saved prior branch:
                        # no unavailable zone-modifier reconstruction or extra control changes.
                        plan=plan_swing(candidate,lever,.16)
                        pf.write(json.dumps(dict(game_id=row['game_id'],count_seq=row['count_seq'],condition=condition,
                            sensor_sigma_m=sigma,eye=p['batting_eye'],observations=observations,estimates=estimates,plan=plan),allow_nan=False)+'\n')
                        motion=FrameMotion(state,lever,.16,plan['pivot_world_m'],plan['pivot_velocity_world_ms'],
                                           plan['actual_axis_world'],plan['actual_omega_world_rad_s'])
                        trial=copy.deepcopy(state)
                        trial['input']['timing_offset_sec']=f['time_sec']-truth['reference_time_sec_since_release']
                        r=result_record(row,trial,motion,f'{condition}_r{lever:g}',
                            max(-.06,-truth['reference_time_sec_since_release']),.06,1e-6,4000,quality)
                        r['linear_infinite_status']='not_applicable';r['linear_infinite_entry_offset_ms']=None
                        e=error_row(row,truth,f,condition,sigma,p['batting_eye'])
                        r.update({k:e[k] for k in EXTRA});writer.writerow(r);summary.add(r);results[condition]=r
                        counts['trial_rows']+=1
                    for condition in results:
                        if condition!='prior_only':paired.add(kind,results[condition]['legacy_group'],lever,
                            'prior_to_'+condition,results['prior_only'],results[condition])
        coverage=dict(first_game_id=first,last_game_id=last,swings=counts['swings'],valid_histories=counts['valid_histories'],
            variants_per_history=(2+len(sigmas))*len(levers),trial_rows=counts['trial_rows'])
        write_csv(output/'observation_update_coverage.csv',list(coverage),[coverage])
        write_csv(output/'observation_update_status.csv',['adjusted_contact_type','status','states'],
            [dict(adjusted_contact_type=k[0],status=k[1],states=n) for k,n in sorted(statuses.items())])
        write_csv(output/'observation_update_forecasts.csv',ERROR_FIELDS,errors)
        write_csv(output/'observation_update_eye_sensitivity.csv',ERROR_FIELDS,sensitivity)
        write_csv(output/'observation_update_error_summary.csv',ERROR_SUMMARY_FIELDS,error_summary(errors))
        write_csv(output/'observation_update_eye_summary.csv',ERROR_SUMMARY_FIELDS,error_summary(sensitivity))
        write_csv(output/'observation_update_summary.csv',SUMMARY_FIELDS+['avg_normal_speed_fraction','ev_ge_40_pct'],summary.rows())
        paired.write(output,prefix='observation_update');quality.write(output)
        (output/'observation_update_metadata.json').write_text(json.dumps(dict(version=1,sensor_source='exact_simulated_three_position_history',
            observation_ratios=[.2,.4,.6],sensor_sigma_m=list(sigmas),eye_scale='exp(-0.5*clamp(eye,-4,4))',
            noise='local_random.Random_sha256_observation_sensor_v1_game_count_common_standard_normals',
            estimator='three_position_quadratic_xz_full_interval_linear_y',levers_m=list(levers),half_length_m=.16,
            half_window_sec=.06,radius_m=.07,restitution=.5,effective_mass_kg=.6,
            execution_angle='saved_prior_error_fixed_across_conditions',
            note='Uncalibrated sensor sensitivity experiment; exact observations control is not a human model. No live RNG or DB writes.'),indent=2)+'\n')
        return coverage
    finally:db.close()


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    add_db_argument(parser);parser.add_argument('--first-game-id',type=int,required=True)
    parser.add_argument('--last-game-id',type=int,required=True);parser.add_argument('--output-dir',type=Path,required=True)
    parser.add_argument('--sensor-sigma-m',type=float,nargs='+',default=[.002,.005,.01])
    parser.add_argument('--levers-m',type=float,nargs='+',default=[.6,.9,1.2]);args=parser.parse_args()
    try:print(run(args.db,args.first_game_id,args.last_game_id,args.output_dir,args.sensor_sigma_m,args.levers_m))
    except (OSError,ValueError,sqlite3.Error) as exc:parser.exit(1,f'Diagnostic failed: {exc}\n')
