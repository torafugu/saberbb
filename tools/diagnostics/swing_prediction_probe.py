#!/usr/bin/env python3
"""Read saved commit observations/forecasts and compare forecast-led swing plans."""
import argparse
from diagnostic_db import add_db_argument
import copy
import csv
import json
import math
import sqlite3
from collections import Counter, defaultdict
from pathlib import Path

from contact_3d import CONTRACT_VERSION, QualityAccumulator
from contact_collision_probe import cross, dot, write_csv
from contact_state_probe import SOURCE, read_rows, validate_state
from execution_frame_probe import FrameMotion, Paired, SummaryAccumulator, frames
from rotating_bat_probe import FIELDS, SUMMARY_FIELDS, add, sub, mul, norm, rotate, result_record

PREDICTION_SOURCE='expected_pitch_prior_commit_observation_v1'


def vector(v):
    return len(v)==3 and all(isinstance(x,(int,float)) and math.isfinite(x) for x in v)


def angle_for_position(position):
    return max(10.,min(60.,30.-15.*((position[2]-.825)/.325)))


def forecast(observation,estimated_velocity,estimated_acceleration):
    """No actual plate position/time or actual contact result is accepted."""
    t=observation['time_sec'];p=observation['position_world_m'];v=estimated_velocity;a=estimated_acceleration
    if (not math.isfinite(t) or t<0 or not all(vector(x) for x in (p,v,a)) or
        p[1]<=0 or v[1]>=0 or abs(a[1])>1e-12):raise ValueError('invalid_forecast_input')
    dt=-p[1]/v[1]
    time=t+dt;position=tuple(x+y*dt+.5*z*dt*dt for x,y,z in zip(p,v,a))
    if not math.isfinite(time) or not vector(position):raise ValueError('non_finite_forecast')
    return dict(time_sec=time,position_world_m=position)


def validate_prediction(record):
    if record['version']!=1 or record['source']!=PREDICTION_SOURCE:raise ValueError('unsupported_prediction')
    if record['invalid_reason'] is not None:raise ValueError('invalid:'+record['invalid_reason'])
    p=record['input']
    calculated=forecast(p['observation'],p['estimated_velocity_world_ms'],p['estimated_acceleration_world_ms2'])
    scalar_fields=('prior_flight_time_sec','prior_speed_ms','commit_ratio','planned_bat_angle_deg',
        'planned_attack_angle_deg','execution_angle_error_deg','bat_speed_ms','handedness_sign',
        'batting_eye','bat_control','zone_modifier_at_prediction','residual_execution_delay_sec')
    if not all(isinstance(p[k],(int,float)) and math.isfinite(p[k]) for k in scalar_fields):raise ValueError('invalid_plan_scalar')
    if (p['prior_flight_time_sec']<=0 or p['prior_speed_ms']<=0 or p['commit_ratio']!=.6 or
        p['bat_speed_ms']<0 or abs(p['planned_attack_angle_deg'])>90 or p['handedness_sign'] not in (-1,1) or
        p['residual_execution_delay_sec']!=0):raise ValueError('invalid_plan_parameters')
    if not math.isfinite(p['forecast']['time_sec']) or not vector(p['forecast']['position_world_m']):raise ValueError('invalid_saved_forecast')
    predicted_angle=angle_for_position(calculated['position_world_m'])
    # Reproduce the existing deterministic angle-error rule at the PREDICTED course.
    factor=p['bat_control']/20*(1-p['zone_modifier_at_prediction'])
    location=list(calculated['position_world_m']);location[2]=.825+(location[2]-.825)*(1+factor)
    intended_angle=angle_for_position(location)
    x=p['bat_control'];sigmoid=(1/(1+math.exp(-x))) if x>=0 else math.exp(x)/(1+math.exp(x))
    error=(intended_angle-predicted_angle)*max(.1,min(1.,1-sigmoid))
    checks=dict(forecast_time_error_sec=abs(calculated['time_sec']-p['forecast']['time_sec']),
        forecast_position_error_m=norm(sub(calculated['position_world_m'],p['forecast']['position_world_m'])),
        commit_time_error_sec=abs(p['observation']['time_sec']-p['prior_flight_time_sec']*.6),
        planned_angle_error_deg=abs(predicted_angle-p['planned_bat_angle_deg']),
        execution_angle_error_deg=abs(error-p['execution_angle_error_deg']))
    if max(checks.values())>1e-8:raise ValueError('saved_prediction_reconstruction_mismatch')
    return p,checks


def plan_swing(prediction,lever,half_length):
    """Forecast and batter configuration only; no actual trajectory is accessible here."""
    p=prediction;target=tuple(p['forecast']['position_world_m']);h=p['handedness_sign']
    theta=math.radians(p['planned_bat_angle_deg']);attack=math.radians(p['planned_attack_angle_deg'])
    axis0=(h*math.cos(theta),0.,math.sin(theta))
    velocity0=(0.,p['bat_speed_ms']*math.cos(attack),p['bat_speed_ms']*math.sin(attack))
    pivot=sub(target,mul(axis0,lever));vp=mul(axis0,dot(velocity0,axis0))
    omega0=mul(cross(axis0,sub(velocity0,vp)),1/lever)
    rotation=(0.,-h*math.radians(p['execution_angle_error_deg']),0.)
    axis1=rotate(axis0,rotation,1.);omega1=rotate(omega0,rotation,1.)
    point1=add(pivot,mul(axis1,lever));velocity1=add(vp,cross(omega1,mul(axis1,lever)))
    return dict(plan_version=1,source=PREDICTION_SOURCE,lever_m=lever,half_length_m=half_length,
        nominal_time_sec=p['forecast']['time_sec']+p['residual_execution_delay_sec'],
        forecast_position_world_m=target,pivot_world_m=pivot,pivot_velocity_world_ms=vp,
        ideal_axis_world=axis0,actual_axis_world=axis1,ideal_point_world_m=target,actual_point_world_m=point1,
        ideal_velocity_world_ms=velocity0,actual_velocity_world_ms=velocity1,
        ideal_omega_world_rad_s=omega0,actual_omega_world_rad_s=omega1,
        execution_rotation_world_rad=rotation,execution_angle_error_deg=p['execution_angle_error_deg'])


EXTRA=['planning_condition','forecast_error_x_m','forecast_error_z_m','forecast_time_error_ms',
    'forecast_time_sec','observation_time_sec','nominal_swing_time_sec','residual_execution_delay_ms',
    'batting_eye','bat_control']


def run(db_path,first,last,output,levers=(.6,.9,1.2),window=.06,half_length=.16,tolerance=1e-6,max_iterations=4000):
    if (first>last or not levers or len(set(levers))!=len(levers) or
        not all(math.isfinite(x) and x>half_length for x in levers) or
        not math.isfinite(window) or window<=0 or not math.isfinite(half_length) or half_length<=0 or
        not math.isfinite(tolerance) or not 0<tolerance<.001 or max_iterations<1):raise ValueError('invalid_parameters')
    output=Path(output);db=sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro',uri=True);db.row_factory=sqlite3.Row
    try:
        db.execute('PRAGMA query_only=ON');db.execute('BEGIN');output.mkdir(parents=True,exist_ok=False)
        counts=Counter();statuses=Counter();checks=defaultdict(float);summary=SummaryAccumulator();paired=Paired();quality=QualityAccumulator()
        forecast_rows=[]
        with (output/'swing_plan_contacts.csv').open('x',newline='',encoding='utf-8') as contact_file,\
             (output/'swing_plan_inputs.jsonl').open('x',encoding='utf-8') as plan_file:
            writer=csv.DictWriter(contact_file,fieldnames=FIELDS+EXTRA);writer.writeheader()
            for row in read_rows(db,first,last):
                counts['swings']+=1;kind=row['adjusted_contact_type']
                if row['state_version'] is None:reason='missing_contact_state'
                elif row['invalid_reason'] is not None:reason='invalid_state:'+row['invalid_reason']
                elif row['state_version']!=1 or row['state_source']!=SOURCE:reason='unsupported_contact_state'
                else:
                    try:state=json.loads(row['state_json']);validate_state(state);reason=None
                    except (ValueError,TypeError,KeyError,OverflowError):reason='invalid_contact_payload'
                if reason:statuses[kind,reason]+=1;continue
                counts['valid_contact_states']+=1
                prediction=state.get('swing_prediction')
                if prediction is None:statuses[kind,'missing_prediction']+=1;continue
                try:
                    p,errors=validate_prediction(prediction)
                    if p['observation']['time_sec']>=state['input']['reference_time_sec_since_release']:raise ValueError('commit_at_or_after_actual_plate')
                    plans=[plan_swing(p,lever,half_length) for lever in levers]
                    controls=[frames(state,lever,half_length)[0][2][1] for lever in levers]
                except (ValueError,TypeError,KeyError,OverflowError) as exc:
                    statuses[kind,'invalid_prediction:'+str(exc)]+=1;continue
                counts['valid_predictions']+=1;statuses[kind,'valid_prediction']+=1
                for key,value in errors.items():checks[key]=max(checks[key],value)
                truth=state['input'];error=sub(p['forecast']['position_world_m'],truth['ball_position_world_m'])
                dt=p['forecast']['time_sec']-truth['reference_time_sec_since_release']
                forecast_rows.append(dict(game_id=row['game_id'],count_seq=row['count_seq'],adjusted_contact_type=kind,
                    observation_time_sec=p['observation']['time_sec'],forecast_time_sec=p['forecast']['time_sec'],
                    actual_plate_time_sec=truth['reference_time_sec_since_release'],forecast_error_x_m=error[0],
                    forecast_error_z_m=error[2],forecast_time_error_ms=dt*1000,batting_eye=p['batting_eye'],bat_control=p['bat_control']))
                for lever,plan,control in zip(levers,plans,controls):
                    plan_file.write(json.dumps(dict(game_id=row['game_id'],count_seq=row['count_seq'],**plan),allow_nan=False)+'\n')
                    planned_motion=FrameMotion(state,lever,half_length,plan['pivot_world_m'],plan['pivot_velocity_world_ms'],
                        plan['actual_axis_world'],plan['actual_omega_world_rad_s'])
                    residual=p['residual_execution_delay_sec'];results={}
                    conditions=[('actual_anchor_control',control,truth['timing_offset_sec']),
                        ('forecast_oracle_time',planned_motion,residual),
                        ('forecast_position_time',planned_motion,dt+residual)]
                    for condition,motion,delay in conditions:
                        trial=copy.deepcopy(state);trial['input']['timing_offset_sec']=delay
                        variant=f'{condition}_r{lever:g}'
                        r=result_record(row,trial,motion,variant,max(-window,-truth['reference_time_sec_since_release']),
                            window,tolerance,max_iterations,quality)
                        r['linear_infinite_status']='not_applicable';r['linear_infinite_entry_offset_ms']=None
                        r.update(planning_condition=condition,forecast_error_x_m=error[0],forecast_error_z_m=error[2],
                            forecast_time_error_ms=dt*1000,forecast_time_sec=p['forecast']['time_sec'],
                            observation_time_sec=p['observation']['time_sec'],nominal_swing_time_sec=truth['reference_time_sec_since_release']+delay,
                            residual_execution_delay_ms=residual*1000,batting_eye=p['batting_eye'],bat_control=p['bat_control'])
                        writer.writerow(r);summary.add(r);results[condition]=r
                    legacy=results['actual_anchor_control']['legacy_group']
                    paired.add(kind,legacy,lever,'actual_anchor_to_forecast',results['actual_anchor_control'],results['forecast_position_time'])
                    paired.add(kind,legacy,lever,'oracle_time_to_forecast_time',results['forecast_oracle_time'],results['forecast_position_time'])
        coverage=dict(first_game_id=first,last_game_id=last,swings=counts['swings'],valid_contact_states=counts['valid_contact_states'],
            valid_predictions=counts['valid_predictions'],variants_per_prediction=3*len(levers),
            trial_rows=counts['valid_predictions']*3*len(levers))
        write_csv(output/'swing_plan_coverage.csv',list(coverage),[coverage])
        write_csv(output/'swing_plan_status.csv',['adjusted_contact_type','status','states'],
            [dict(adjusted_contact_type=k[0],status=k[1],states=n) for k,n in sorted(statuses.items())])
        write_csv(output/'swing_plan_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
        write_csv(output/'swing_plan_summary.csv',SUMMARY_FIELDS+['avg_normal_speed_fraction','ev_ge_40_pct'],summary.rows())
        fields=['game_id','count_seq','adjusted_contact_type','observation_time_sec','forecast_time_sec','actual_plate_time_sec',
            'forecast_error_x_m','forecast_error_z_m','forecast_time_error_ms','batting_eye','bat_control']
        write_csv(output/'swing_plan_forecasts.csv',fields,forecast_rows)
        paired.write(output,prefix="swing_plan");quality.write(output)
        (output/'swing_plan_metadata.json').write_text(json.dumps(dict(plan_version=1,prediction_source=PREDICTION_SOURCE,
            contact_contract_version=CONTRACT_VERSION,levers_m=list(levers),half_length_m=half_length,effective_radius_m=.07,
            half_window_sec=window,tolerance_m=tolerance,max_iterations=max_iterations,restitution=.5,effective_mass_kg=.6,
            residual_execution_delay_sec=0,prior='existing_independent_expected_ball',
            observation='exact_simulated_position_at_0.6_times_prior_flight_time',
            note='Uncalibrated diagnostic forecast; oracle time/actual-anchor branches are evaluation controls only'),indent=2)+'\n')
        return coverage
    finally:db.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    add_db_argument(p);p.add_argument('--first-game-id',type=int,required=True)
    p.add_argument('--last-game-id',type=int,required=True);p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--levers-m',type=float,nargs='+',default=[.6,.9,1.2]);p.add_argument('--half-window-ms',type=float,default=60)
    p.add_argument('--half-length-m',type=float,default=.16);p.add_argument('--contact-tolerance-m',type=float,default=1e-6)
    p.add_argument('--max-iterations',type=int,default=4000);a=p.parse_args()
    try:print(run(a.db,a.first_game_id,a.last_game_id,a.output_dir,a.levers_m,a.half_window_ms/1000,a.half_length_m,a.contact_tolerance_m,a.max_iterations))
    except (OSError,ValueError,sqlite3.Error) as exc:p.exit(1,f'Diagnostic failed: {exc}\n')
