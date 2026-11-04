#!/usr/bin/env python3
"""Outcome-independent sampled sweep of intended bat-axis inclination; diagnostic only."""
import argparse
import csv
import json
import math
import hashlib
import sqlite3
from collections import Counter,defaultdict
from pathlib import Path
from itertools import product

from bat_axis_origin_audit import check, inspect, tilt
from bat_pivot_orientation_probe import Aggregate, evaluate, MODEL_NAMES, KEYS
from contact_collision_probe import write_csv
from contact_3d import band
from effective_mass_probe import direction
from contact_state_probe import SOURCE,validate_state
from diagnostic_db import add_db_argument
from execution_contact_posture import source_rows,digest,same
from finite_barrel_probe import event,configs
from forecast_contact_geometry import audit,vec
from pitch_spin_source import PitchSpinSource
from rotating_bat_probe import norm,sub,add,mul
from swing_execution import SwingPlan,ExecutionDeviation,PlanExecutionMotion,SwingExecution
from swing_execution_probe import write_aggregates
from planned_axis import plan_axis_swing,with_planned_speed

KINEMATICS=('planned_tilt_deg','reference_axis_elevation_deg','planned_speed_ms','reference_point_speed_ms',
    'reference_pivot_z_m','start_pivot_z_m','pulse_min_pivot_z_m','pulse_max_pivot_z_m',
    'start_point_z_m','reference_target_error_m','peak_omega_rad_s')


def sample_keys(path,variant,size,seed,excluded_keys=None):
    if isinstance(size,bool) or not isinstance(size,int) or size<0:raise ValueError('invalid_sample_size')
    ranks={}
    with Path(path).open(newline='') as f:
        reader=csv.DictReader(f)
        if not {'game_id','count_seq','variant'}<=set(reader.fieldnames or []):raise ValueError('missing_sample_key_columns')
        for row in reader:
            if row['variant']!=variant:continue
            key=(int(row['game_id']),int(row['count_seq']))
            if min(key)<0 or key in ranks:raise ValueError('invalid_or_duplicate_population_key')
            ranks[key]=hashlib.sha256(f'plan-tilt-v1:{seed}:{key[0]}:{key[1]}'.encode()).hexdigest()
    if not ranks:raise ValueError('empty_sample_population')
    excluded=set(excluded_keys or ())
    if excluded-set(ranks):raise ValueError('exclusion_contains_unknown_population_keys')
    ranks={k:v for k,v in ranks.items() if k not in excluded}
    if not ranks:raise ValueError('empty_population_after_exclusion')
    ordered=sorted(ranks,key=lambda k:(ranks[k],k))
    selected=ordered if size==0 else ordered[:size]
    return {key:ranks[key] for key in selected},len(ranks)


def exclusion_manifest(path):
    if path is None:return None
    keys=set()
    with Path(path).open(newline='') as f:
        reader=csv.DictReader(f)
        if not {'game_id','count_seq'}<=set(reader.fieldnames or []):raise ValueError('missing_exclusion_columns')
        for r in reader:
            key=int(r['game_id']),int(r['count_seq'])
            if min(key)<0 or key in keys:raise ValueError('invalid_or_duplicate_exclusion_key')
            keys.add(key)
    if not keys:raise ValueError('empty_exclusion_manifest')
    return dict(path=str(path),sha256=digest(path),excluded_count=len(keys),excluded_keys=sorted(keys))


def replan(saved,angle,checks,yaw=0.,aim_z_m=None):
    """Use only the original forecast, intended speed/attack, handedness and angular error."""
    velocity=saved['ideal_velocity_world_ms']
    check(checks,'planned_lateral_velocity_ms',abs(velocity[0]))
    prediction=dict(forecast=dict(position_world_m=saved['forecast_position_world_m'],time_sec=saved['nominal_time_sec']),
        handedness_sign=1 if saved['ideal_axis_world'][0]>0 else -1,
        planned_bat_angle_deg=angle,planned_attack_angle_deg=math.degrees(math.atan2(velocity[2],velocity[1])),
        bat_speed_ms=norm(velocity),execution_angle_error_deg=saved['execution_angle_error_deg'],residual_execution_delay_sec=0.)
    result=plan_axis_swing(prediction,saved['lever_m'],saved['half_length_m'],yaw,
        None if aim_z_m is None else (0.,0.,aim_z_m))
    if aim_z_m is not None:
        expected=add(saved['forecast_position_world_m'],(0.,0.,aim_z_m))
        same(expected,result['planned_aim_point_world_m'],checks,'planned_aim_point')
        same(expected,result['ideal_point_world_m'],checks,'ideal_planned_aim')
    same(saved['forecast_position_world_m'],result['forecast_position_world_m'],checks,'common_forecast')
    same(saved['nominal_time_sec'],result['nominal_time_sec'],checks,'common_nominal_time')
    same(velocity,result['ideal_velocity_world_ms'],checks,'common_planned_velocity')
    same(saved['execution_angle_error_deg'],result['execution_angle_error_deg'],checks,'common_execution_error')
    return result


def kinematics(saved,plan,motion):
    end=add(plan.start_pivot_world_m,mul(plan.peak_pivot_velocity_world_ms,plan.duration_sec/2))
    return dict(planned_tilt_deg=math.degrees(math.asin(max(-1,min(1,saved['ideal_axis_world'][2])))),
        reference_axis_elevation_deg=math.degrees(math.asin(max(-1,min(1,plan.reference_axis_world[2])))),
        planned_speed_ms=norm(saved['ideal_velocity_world_ms']),reference_point_speed_ms=norm(motion.velocity_at_phase(0.,0.)),
        reference_pivot_z_m=saved['pivot_world_m'][2],start_pivot_z_m=plan.start_pivot_world_m[2],
        pulse_min_pivot_z_m=min(plan.start_pivot_world_m[2],end[2]),pulse_max_pivot_z_m=max(plan.start_pivot_world_m[2],end[2]),
        start_point_z_m=plan.start_point_world_m[2],reference_target_error_m=norm(sub(plan.reference_point_world_m,plan.forecast_position_world_m)),
        peak_omega_rad_s=norm(plan.peak_omega_world_rad_s))


def run(input_dir,db_path,output,variant='observed_sigma_0.005_r0.9',angles=(0.,15.,30.),sample_size=2000,
        sample_seed='0',duration=.18,fraction=.5,gyro_sign=0,yaws=None,aim_z_offsets=None,speed_ratios=None,exclude_sample=None):
    if (not angles or len(set(angles))!=len(angles) or any(not math.isfinite(x) or not 0<=x<=60 for x in angles)
        or gyro_sign not in (-1,0,1)):raise ValueError('invalid_tilt_scenarios')
    if yaws is not None and (not yaws or len(set(yaws))!=len(yaws) or any(not math.isfinite(y) or abs(y)>45 for y in yaws)):raise ValueError('invalid_yaw_scenarios')
    if aim_z_offsets is not None and (not aim_z_offsets or len(set(aim_z_offsets))!=len(aim_z_offsets)
        or any(not math.isfinite(z) for z in aim_z_offsets)):raise ValueError('invalid_aim_z_scenarios')
    if speed_ratios is not None and (not speed_ratios or len(set(speed_ratios))!=len(speed_ratios)
        or any(isinstance(r,bool) or not isinstance(r,(int,float)) or not math.isfinite(r) or not 0<r<=1 for r in speed_ratios)
        or yaws is None or aim_z_offsets is None):raise ValueError('invalid_planned_speed_scenarios')
    speed_values=(None,) if speed_ratios is None else speed_ratios
    yaw_values=(0.,) if yaws is None else yaws
    aim_values=(None,) if aim_z_offsets is None else aim_z_offsets
    names={}
    for x,y,z,r in product(angles,yaw_values,aim_values,speed_values):
        name=(f'tilt_{x:g}deg_yaw_{y:g}deg_aim_z_{z*1000:g}mm' if z is not None else
              f'tilt_{x:g}deg_yaw_{y:g}deg' if yaws is not None else f'tilt_{x:g}deg')
        if r is not None:name+=f'_speed_r{r:g}'
        if name in names:raise ValueError('tilt_label_collision')
        names[name]=(x,y,z,r)
    kinematic_fields=KINEMATICS+(('planned_axis_yaw_batter_deg','reference_axis_yaw_batter_deg') if yaws is not None or aim_z_offsets is not None else ())
    if aim_z_offsets is not None:
        kinematic_fields+=('planned_aim_z_offset_m','planned_aim_z_m','reference_aim_error_m')
    if speed_ratios is not None:kinematic_fields+=('planned_speed_ratio',)
    SwingExecution(0,0,10.,1.,duration,fraction,0.)
    input_dir=Path(input_dir);output=Path(output)
    exclusion=exclusion_manifest(exclude_sample)
    selected,population=sample_keys(input_dir/'observation_update_contacts.csv',variant,sample_size,str(sample_seed),
                                   exclusion['excluded_keys'] if exclusion else None)
    output.mkdir(parents=True,exist_ok=False)
    write_csv(output/'plan_tilt_sample.csv',['game_id','count_seq','selection_hash'],[
        dict(game_id=k[0],count_seq=k[1],selection_hash=v) for k,v in selected.items()])
    db=sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro',uri=True);spin=None
    try:
        db.execute('PRAGMA query_only=ON');db.execute('BEGIN')
        spin=PitchSpinSource(db_path,gyro_sign,output/'invalid_pitch_spin.csv')
        groups={k:Aggregate('saved_tilt','planned_tilt') for k in names}
        metrics=defaultdict(Counter);checks=defaultdict(float);counts=Counter(new_searches=0);seen=set()
        models=[m for m in configs((.00015,)) if m['name'] in MODEL_NAMES]
        with (output/'plan_tilt_states.jsonl').open('x') as sf:
            for record,raw in source_rows(input_dir,variant,selected):
                row=audit(raw,checks);source=record['plan'];inspect(source,row,checks)
                key=(row['game_id'],row['count_seq'])
                if key in seen:raise ValueError('duplicate_sample_pitch')
                seen.add(key);counts['sampled_trials']+=1
                found=db.execute('SELECT state_version,state_source,invalid_reason,state_json FROM player_game_contact_state WHERE game_id=? AND count_seq=?',key).fetchall()
                if len(found)!=1 or found[0][:3]!=(1,SOURCE,None):raise ValueError('invalid_or_missing_contact_state')
                state=json.loads(found[0][3]);validate_state(state);truth=state['input']
                if truth['handedness_sign']!=(1 if source['ideal_axis_world'][0]>0 else -1):raise ValueError('db_handedness_mismatch')
                for i,k in ((0,'x'),(2,'z')):
                    check(checks,'db_forecast_'+k,abs(source['forecast_position_world_m'][i]-truth['ball_position_world_m'][i]-row['forecast_error_'+k+'_m']))
                check(checks,'db_clock_sec',abs(source['nominal_time_sec']-truth['reference_time_sec_since_release']-row['forecast_time_error_ms']/1000))
                if row['status']=='contact':check(checks,'db_ball_velocity_ms',norm(sub(vec(row,'ball_velocity_'),truth['ball_velocity_world_ms'])))
                incoming=spin.get(dict(game_id=key[0],count_seq=key[1],variant=variant,
                    **{'ball_velocity_'+k+'_ms':v for k,v in zip('xyz',truth['ball_velocity_world_ms'])}))
                available=max(o['time_sec'] for o in record['observations']) if record.get('observations') else state['swing_prediction']['input']['observation']['time_sec']
                if not math.isfinite(available) or not 0<=available<source['nominal_time_sec']:raise ValueError('invalid_plan_available_clock')
                # Recovering the original angle must reproduce the entire original reference plan.
                source_angle=math.degrees(math.atan2(source['ideal_axis_world'][2],abs(source['ideal_axis_world'][0])))
                same(source,replan(source,source_angle,checks),checks,'saved_plan_identity')
                plans={'saved_tilt':source}
                for name,(x,y,z,r) in names.items():
                    candidate=replan(source,x,checks,y,z)
                    plans[name]=with_planned_speed(candidate,r) if r is not None else candidate
                    if r is not None:
                        same(mul(candidate['ideal_velocity_world_ms'],r),plans[name]['ideal_velocity_world_ms'],checks,'scaled_planned_velocity')
                scopes=['all_swings']+(['recorded_batting_only'] if row['adjusted_contact_type']!='UnrecordedBatting' else [])
                outcomes={};plan_records={}
                for name,saved in plans.items():
                    invalid=None
                    try:plan=SwingPlan.from_saved(saved,1.,duration,fraction)
                    except ValueError as exc:
                        if str(exc)!='execution_starts_before_release':raise
                        invalid=str(exc)
                    if invalid is None and plan.start_time_sec<available-1e-10:invalid='execution_starts_before_plan'
                    for scope in scopes:
                        g=metrics[name,scope];g['trials']+=1;g['clock_invalid_trials']+=invalid is not None
                    if invalid:
                        changed=dict(row,status=invalid,collision_status='not_evaluated:'+invalid)
                        results={m:dict(collision_status='not_evaluated:'+invalid,ev_ms=None) for m in MODEL_NAMES}
                        counts[name+':'+invalid]+=1
                        sf.write(json.dumps(dict(scenario=name,game_id=key[0],count_seq=key[1],invalid_reason=invalid,forecast_plan=saved))+'\n')
                    else:
                        motion=PlanExecutionMotion(state,plan,ExecutionDeviation(),*key)
                        point,axis=motion.pose(0.,0.)
                        check(checks,'reference_position_m',norm(sub(point,plan.reference_point_world_m)))
                        check(checks,'reference_axis',norm(sub(axis,plan.reference_axis_world)))
                        values=kinematics(saved,plan,motion)
                        if yaws is not None or aim_z_offsets is not None:
                            h=truth['handedness_sign'];a=saved['ideal_axis_world'];b=plan.reference_axis_world
                            values.update(planned_axis_yaw_batter_deg=math.degrees(math.atan2(a[1],h*a[0])),
                                reference_axis_yaw_batter_deg=math.degrees(math.atan2(b[1],h*b[0])))
                        if aim_z_offsets is not None:
                            aim=saved.get('planned_aim_point_world_m',saved['ideal_point_world_m'])
                            values.update(planned_aim_z_offset_m=saved.get('planned_aim_offset_world_m',(0.,0.,0.))[2],
                                planned_aim_z_m=aim[2],reference_aim_error_m=norm(sub(plan.reference_point_world_m,aim)))
                        if speed_ratios is not None:values['planned_speed_ratio']=saved.get('planned_speed_ratio',1.)
                        for scope in scopes:
                            g=metrics[name,scope];g['evaluable_trials']+=1;g['pulse_pivot_below_ground']+=values['pulse_min_pivot_z_m']<0
                            for k,v in values.items():g[k]+=v
                            g['min_pulse_pivot_z_m']=min(g.get('min_pulse_pivot_z_m',math.inf),values['pulse_min_pivot_z_m'])
                            g['max_reference_point_speed_ms']=max(g['max_reference_point_speed_ms'],values['reference_point_speed_ms'])
                        changed=evaluate(row,state,saved,checks,motion=motion);counts['new_searches']+=1
                        results,reason=event(changed,checks,models,incoming,None,include_ground=False)
                        counts[name+':'+reason]+=1
                        sf.write(json.dumps(dict(scenario=name,game_id=key[0],count_seq=key[1],forecast_plan=saved,
                            plan=plan.record(),execution=motion.execution.record(),kinematics=values,
                            entry_time_sec=changed.get('entry_since_release_sec'),search_status=changed['status'],
                            contact=changed,responses=results,incoming_spin_world_rad_s=incoming),allow_nan=False)+'\n')
                    outcomes[name]=(changed,results);plan_records[name]=saved
                before_row,before=outcomes['saved_tilt']
                for name,aggregate in groups.items():
                    after_row,after=outcomes[name]
                    aggregate.add(row,dict(saved_tilt=source,planned_tilt=plan_records[name]),
                        dict(saved_tilt=before_row,planned_tilt=after_row),dict(saved_tilt=before,planned_tilt=after))
                    for scope in scopes:
                        for stratum,b in [('overall','all'),('planned_tilt',band(tilt(source['ideal_axis_world']),(10,20,30,40,50,60,75,90)))]:
                            for orientation,results in [('saved_tilt',before),('planned_tilt',after)]:
                                for model,m in results.items():
                                    if direction(m)!='fair_direction':continue
                                    g=aggregate.summary[scope,variant,stratum,b,orientation,model]
                                    g['fair_low_ev']+=m['ev_ms']<25;g['fair_high_ev']+=m['ev_ms']>=45
                                    g['fair_downward']+=m['vla_deg']<0;g['fair_high_vla']+=m['vla_deg']>=50
                                    hla=m['hla_deg'];bh=truth['handedness_sign']*hla
                                    g['fair_abs_hla_deg']+=abs(hla);g['fair_batter_hla_deg']+=bh;g['fair_central10']+=abs(hla)<10
                                    if m['vla_deg']>=50:
                                        g['fair_high_vla_negative_batter_hla']+=bh<0
                                        g['fair_high_vla_zero_batter_hla']+=bh==0
                                        g['fair_high_vla_positive_batter_hla']+=bh>0
        if seen!=set(selected):raise ValueError('missing_sample_source_rows')
        expected=sum(g['evaluable_trials'] for (name,scope),g in metrics.items() if scope=='all_swings')
        if counts['new_searches']!=expected:raise ValueError('search_count_mismatch')
        write_aggregates(output,groups,'plan_tilt')
        # Add fair-only region counts to this diagnostic's summary without changing shared output contracts.
        path=output/'plan_tilt_summary.csv'
        with path.open(newline='') as f:
            reader=csv.DictReader(f);fields=reader.fieldnames;summary_rows=list(reader)
        fair_fields=['fair_low_ev','fair_high_ev','fair_downward','fair_high_vla',
            'fair_high_vla_negative_batter_hla','fair_high_vla_zero_batter_hla','fair_high_vla_positive_batter_hla']
        direction_fields=['fair_avg_abs_hla_deg','fair_avg_batter_hla_deg','fair_central_abs_lt10_fraction']
        for r in summary_rows:
            g=groups[r['scenario']].summary[tuple(r[k] for k in KEYS)]
            r.update({k:g[k] for k in fair_fields})
            for label,metric in zip(direction_fields,('fair_abs_hla_deg','fair_batter_hla_deg','fair_central10')):
                r[label]=g[metric]/g['fair_contacts'] if g['fair_contacts'] else None
        with path.open('w',newline='') as f:
            writer=csv.DictWriter(f,fieldnames=fields+fair_fields+direction_fields);writer.writeheader();writer.writerows(summary_rows)
        rows=[]
        for (name,scope),g in sorted(metrics.items()):
            n=g['evaluable_trials'];r=dict(scenario=name,scope=scope,trials=g['trials'],evaluable_trials=n,
                clock_invalid_trials=g['clock_invalid_trials'],pulse_pivot_below_ground=g['pulse_pivot_below_ground'],
                min_pulse_pivot_z_m=g.get('min_pulse_pivot_z_m') if n else None,
                max_reference_point_speed_ms=g['max_reference_point_speed_ms'] if n else None)
            for k in kinematic_fields:r['avg_'+k]=g[k]/n if n else None
            rows.append(r)
        fields=['scenario','scope','trials','evaluable_trials','clock_invalid_trials','pulse_pivot_below_ground',
                'min_pulse_pivot_z_m','max_reference_point_speed_ms']+['avg_'+k for k in kinematic_fields]
        write_csv(output/'plan_tilt_kinematics.csv',fields,rows)
        write_csv(output/'plan_tilt_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
        write_csv(output/'plan_tilt_coverage.csv',['metric','trials'],[dict(metric=k,trials=v) for k,v in sorted(counts.items())])
        paths=[input_dir/f for f in ('observation_update_contacts.csv','observation_update_inputs.jsonl')]+[output/'plan_tilt_sample.csv']
        metadata=dict(version=4 if speed_ratios is not None else 3 if aim_z_offsets is not None else 1 if yaws is None else 2,status='sampled_sensitivity_not_calibrated',variant=variant,models=list(MODEL_NAMES),
            population_trials=population,sampled_trials=len(selected),requested_sample_size=sample_size,sample_seed=str(sample_seed),
            sampling='lowest SHA256(plan-tilt-v1:seed:game_id:count_seq); all matching-variant pitches, before clock/contact/outcome selection; zero sample-size means full population',
            angles_deg=list(angles),**(dict(yaws_deg=list(yaw_values),kinematics_fields=list(kinematic_fields),
                scenarios={name:dict(tilt_deg=x,yaw_batter_deg=y,**(dict(aim_z_offset_m=z) if z is not None else {}),**(dict(planned_speed_ratio=r) if r is not None else {})) for name,(x,y,z,r) in names.items()},
                yaw_convention='World Z rotation = handedness_sign * planned yaw. Saved inclination error rotates with this local plane; yaw is not output HLA.') if yaws is not None or aim_z_offsets is not None else {}),
            **(dict(aim_z_offsets_m=list(aim_z_offsets),aim_convention='Intended bat reference = predicted ball center + world-z offset; negative is below. Forecast/time are unchanged; velocity changes only with explicit planned_speed_ratios.') if aim_z_offsets is not None else {}),
            **(dict(planned_speed_ratios=list(speed_ratios),speed_convention='All intended linear/angular velocities scaled before generating initial pose. Forecast/time/aim/axis/error unchanged; execution_ratio remains 1.') if speed_ratios is not None else {}),
            baseline='original saved planned tilt and zero aim offset, executed with the same sin-squared pulse',
            plan_effort=1.,execution_ratio=1.,start_delay_sec=0.,duration_sec=duration,anchor_fraction=fraction,
            new_searches=counts['new_searches'],maximum_searches=len(selected)*(len(names)+1),spin=spin.metadata(),
            inputs={str(p):digest(p) for p in paths},
            **(dict(sampling_exclusion=exclusion,population_trials_before_exclusion=population+exclusion['excluded_count']) if exclusion else {}),
            limitations=['Angles are explicit intended-plan sensitivity hypotheses, not inferred player choices or recommended values.',
                'Predicted ball center/time and ideal velocity direction are common; intended speed is scaled only when declared; explicit intended aim offset and retained angular execution error can move the actual reference position.',
                'Each intended tilt/yaw generates its own initial pose, then integrates forward; no actual contact used to re-anchor or choose a plan.',
                'Changed axes require new contact searches. Baseline is searched once per sampled pitch and reused across contrasts.',
                'No-saved-contact/clock invalidity/cap/curve failures are not successful EV/VLA cases; compare denominators and paired sets.',
                'Pivot height is a geometric proxy, not a human-body feasibility model. Below-ground cases are audited, not silently filtered.',
                'Scenarios/models/scopes/strata overlap; repeated saved_tilt rows are controls, not additional pitches.',
                'Small sample comparisons are exploratory. No Statcast fitting, production parameter changes, game/RNG/DB writes.'])
        (output/'plan_tilt_metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
        return dict(population_trials=population,sampled_trials=len(selected),new_searches=counts['new_searches'],maximum_searches=metadata['maximum_searches'])
    finally:
        if spin is not None:spin.close()
        db.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,required=True);add_db_argument(p)
    p.add_argument('--output-dir',type=Path,required=True);p.add_argument('--variant',default='observed_sigma_0.005_r0.9')
    p.add_argument('--tilts-deg',type=float,nargs='+',default=[0.,15.,30.]);p.add_argument('--sample-size',type=int,default=2000)
    p.add_argument('--sample-seed',default='0');p.add_argument('--duration-sec',type=float,default=.18)
    p.add_argument('--anchor-fraction',type=float,default=.5);p.add_argument('--gyro-sign',type=int,choices=(-1,0,1),default=0)
    a=p.parse_args()
    try:print(json.dumps(run(a.input_dir,a.db,a.output_dir,a.variant,a.tilts_deg,a.sample_size,a.sample_seed,a.duration_sec,a.anchor_fraction,a.gyro_sign),indent=2))
    except (OSError,ValueError,KeyError,TypeError,sqlite3.Error) as exc:p.exit(1,f'Planned tilt sweep failed: {exc}\n')
