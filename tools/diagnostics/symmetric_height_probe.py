#!/usr/bin/env python3
"""Read-only symmetric vertical aim errors with constant translating bat velocity."""
import argparse
from diagnostic_db import add_db_argument
import copy
import json
import math
import sqlite3
from collections import Counter, defaultdict
from pathlib import Path

from contact_3d import ContactGeometry, solve_contact
from contact_collision_probe import write_csv
from contact_state_probe import SOURCE, read_rows, validate_state
from observation_update_probe import validate_history, estimate
from rotating_bat_probe import Motion, add, sub, mul, norm, rotate, search
from swing_prediction_probe import validate_prediction, plan_swing, angle_for_position

AXES=('fixed_axis','height_dependent_axis')
METRICS=['ev_ms','hla_deg','vla_deg','normal_z','normal_speed_fraction','signed_axial_fraction',
         'entry_time_offset_ms','incoming_z_ms','impulse_z_ms','outgoing_z_ms']
SUMMARY_KEYS=['scope','lever_m','axis_mode','height_error_m']
PAIR_KEYS=['scope','lever_m','axis_mode','height_error_m']


def control_frame(p,base,lever,error,axis_mode):
    """Same anchor shift and WORLD velocity. Only the zero-phase axis rule changes."""
    if axis_mode not in AXES:raise ValueError('invalid_axis_mode')
    point=add(base['actual_point_world_m'],(0.,0.,error))
    if axis_mode=='fixed_axis':axis=tuple(base['actual_axis_world'])
    else:
        target=add(p['forecast']['position_world_m'],(0.,0.,error))
        theta=math.radians(angle_for_position(target));h=p['handedness_sign']
        a=(h*math.cos(theta),0.,math.sin(theta))
        axis=rotate(a,(0.,-h*math.radians(p['execution_angle_error_deg']),0.),1.)
    return dict(point=point,axis=axis,velocity=tuple(base['actual_velocity_world_ms']),
                nominal_time_sec=base['nominal_time_sec'])


def motion_for(state,frame):
    m=Motion(state,None,.16)
    m.point=frame['point'];m.axis=frame['axis'];m.velocity=frame['velocity']
    m.pivot=m.point;m.pivot_velocity=m.velocity;m.omega=(0.,0.,0.)
    m.speed_bound=norm(sub(m.ball_velocity,m.velocity))
    return m


def trial(state,frame,window=.06):
    truth=state['input'];m=motion_for(state,frame)
    delay=frame['nominal_time_sec']-truth['reference_time_sec_since_release']
    event=search(m,delay,max(-window,-truth['reference_time_sec_since_release']),window,1e-6,4000)
    r=dict(search_status=event['status'],collision_status='not_evaluated:'+event['status'],contact_region=None,
           **{k:None for k in METRICS})
    if event['status']!='contact':return r
    normal=event['normal'];r['contact_region']=event['contact_region']
    g=ContactGeometry(truth['reference_time_sec_since_release']+event['time_offset_sec'],tuple(event['axis']),
        tuple(normal),event['axial_m'],event['closest_axial_m'],.16,event['contact_region'])
    metrics=solve_contact(g,m.ball_velocity,frame['velocity'])
    r.update({k:metrics[k] for k in ('collision_status','ev_ms','hla_deg','vla_deg','normal_speed_fraction')})
    r.update(normal_z=normal[2],signed_axial_fraction=event['closest_axial_m']/.16,
             entry_time_offset_ms=event['time_offset_sec']*1000)
    if metrics['outgoing_world_ms'] is not None:
        r.update(incoming_z_ms=m.ball_velocity[2],impulse_z_ms=metrics['impulse_delta_ms']*normal[2],
                 outgoing_z_ms=metrics['outgoing_world_ms'][2])
    return r


def pair_add(g,before,after,zero=None):
    g['states']+=1
    if before['collision_status']!=after['collision_status'] or before['collision_status']!='impulse_contact':return
    g['paired_impulse_contacts']+=1
    triple=zero is not None and zero['collision_status']=='impulse_contact'
    g['triple_impulse_contacts']+=triple
    for k in METRICS:
        if before[k] is None or after[k] is None:continue
        n=k+'_n';g[n]+=1;g[k+'_before']+=before[k];g[k+'_after']+=after[k]
        delta=after[k]-before[k]
        if k=='hla_deg':delta=(delta+180)%360-180
        g[k+'_delta']+=delta;g[k+'_abs_delta']+=abs(delta)
        # Arithmetic HLA midpoints are ambiguous across +/-180; exclude this statistic.
        if triple and zero[k] is not None and k!='hla_deg':
            g[k+'_triple_n']+=1;g[k+'_zero']+=zero[k]
            g[k+'_triple_before']+=before[k];g[k+'_triple_after']+=after[k]
            g[k+'_center_shift']+=(before[k]+after[k])/2-zero[k]


def write_pairs(path,groups):
    rows=[]
    for key,g in sorted(groups.items()):
        r=dict(zip(PAIR_KEYS,key),states=g['states'],paired_impulse_contacts=g['paired_impulse_contacts'],
               triple_impulse_contacts=g['triple_impulse_contacts'])
        for k in METRICS:
            n=g[k+'_n'];t=g[k+'_triple_n'];r['pairs_'+k]=n;r['triples_'+k]=t
            for prefix,suffix in [('before_','_before'),('after_','_after'),('delta_','_delta'),('abs_delta_','_abs_delta')]:
                r[prefix+k]=g[k+suffix]/n if n else None
            r['zero_'+k]=g[k+'_zero']/t if t else None
            r['triple_before_'+k]=g[k+'_triple_before']/t if t else None
            r['triple_after_'+k]=g[k+'_triple_after']/t if t else None
            r['center_shift_'+k]=g[k+'_center_shift']/t if t else None
        rows.append(r)
    write_csv(path,PAIR_KEYS+['states','paired_impulse_contacts','triple_impulse_contacts']+
        [prefix+k for k in METRICS for prefix in ('pairs_','before_','after_','delta_','abs_delta_','triples_','zero_','triple_before_','triple_after_','center_shift_')],rows)


class Aggregate:
    def __init__(self):
        self.summary=defaultdict(Counter);self.symmetry=defaultdict(Counter);self.axes=defaultdict(Counter);self.transitions=Counter()
    def add(self,kind,lever,results,heights):
        scopes=['all_swings']+(['recorded_batting_only'] if kind!='UnrecordedBatting' else [])
        for scope in scopes:
            for (mode,error),r in results.items():
                g=self.summary[scope,lever,mode,error];g['states']+=1;g[r['search_status']]+=1
                g['impulse_contacts']+=r['collision_status']=='impulse_contact'
                g['non_approaching']+=r['collision_status']=='non_approaching';g['cap_contacts']+=r['contact_region']=='cap'
                for k in METRICS:
                    if r[k] is not None:g[k]+=r[k];g[k+'_n']+=1
                if r['vla_deg'] is not None:g['negative_vla']+=r['vla_deg']<0
                if r['hla_deg'] is not None:g['foul']+=abs(r['hla_deg'])>=45
            for mode in AXES:
                for height in heights:
                    a,b,z=results[mode,-height],results[mode,height],results[mode,0.]
                    pair_add(self.symmetry[scope,lever,mode,height],a,b,z)
                    self.transition((scope,lever,'lower_to_upper',mode,height),a,b)
            for error in sorted({e for _,e in results}):
                a,b=results[AXES[0],error],results[AXES[1],error]
                pair_add(self.axes[scope,lever,'fixed_to_height_dependent',error],a,b)
                self.transition((scope,lever,'fixed_to_height_dependent','both',error),a,b)
    def transition(self,key,a,b):
        self.transitions[key+(a['search_status'],b['search_status'],a['collision_status'],b['collision_status'])]+=1
    def write(self,output):
        rows=[]
        for key,g in sorted(self.summary.items()):
            r=dict(zip(SUMMARY_KEYS,key),states=g['states'])
            for k in ('contact','no_contact','overlap_at_start','unresolved','impulse_contacts','non_approaching','cap_contacts'):r[k]=g[k]
            for k in METRICS:r['avg_'+k]=g[k]/g[k+'_n'] if g[k+'_n'] else None
            r.update(defined_hla_contacts=g['hla_deg_n'],defined_vla_contacts=g['vla_deg_n'],
                negative_vla_pct=100*g['negative_vla']/g['vla_deg_n'] if g['vla_deg_n'] else None,
                foul_direction_pct=100*g['foul']/g['hla_deg_n'] if g['hla_deg_n'] else None)
            rows.append(r)
        write_csv(output/'symmetric_height_summary.csv',SUMMARY_KEYS+['states','contact','no_contact','overlap_at_start','unresolved',
            'impulse_contacts','non_approaching','cap_contacts','defined_hla_contacts','defined_vla_contacts',
            'negative_vla_pct','foul_direction_pct']+['avg_'+k for k in METRICS],rows)
        write_pairs(output/'symmetric_height_symmetry_pairs.csv',self.symmetry)
        write_pairs(output/'symmetric_height_axis_pairs.csv',self.axes)
        keys=['scope','lever_m','comparison','axis_mode','height_error_m','before_search_status','after_search_status',
              'before_collision_status','after_collision_status']
        write_csv(output/'symmetric_height_transitions.csv',keys+['states'],[dict(zip(keys,k),states=n) for k,n in sorted(self.transitions.items())])


def run(db_path,first,last,output,heights=(.01,.03,.05),levers=(.6,.9,1.2)):
    if first>last or not heights or len(set(heights))!=len(heights) or any(not math.isfinite(x) or x<=0 for x in heights):
        raise ValueError('invalid_heights_or_range')
    if not levers or len(set(levers))!=len(levers) or any(not math.isfinite(x) or x<=.16 for x in levers):raise ValueError('invalid_levers')
    output=Path(output);db=sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro',uri=True);db.row_factory=sqlite3.Row
    try:
        db.execute('PRAGMA query_only=ON');db.execute('BEGIN');output.mkdir(parents=True,exist_ok=False)
        counts=Counter();statuses=Counter();checks=defaultdict(float);aggregate=Aggregate()
        errors=sorted({0.}|set(heights)|{-x for x in heights})
        for row in read_rows(db,first,last):
            counts['swings']+=1;kind=row['adjusted_contact_type']
            try:
                if row['state_version'] is None:raise ValueError('missing_contact_state')
                if row['state_version']!=1 or row['state_source']!=SOURCE or row['invalid_reason'] is not None:raise ValueError('invalid_contact_state')
                state=json.loads(row['state_json']);validate_state(state)
                if state.get('swing_prediction') is None:raise ValueError('missing_prediction')
                p,_=validate_prediction(state['swing_prediction']);history=validate_history(p)
                if history[-1]['time_sec']>=state['input']['reference_time_sec_since_release']:raise ValueError('history_after_plate')
                f,_,_=estimate(history)
                if (norm(sub(f['position_world_m'],state['input']['ball_position_world_m']))>1e-8 or
                    abs(f['time_sec']-state['input']['reference_time_sec_since_release'])>1e-8):
                    raise ValueError('exact_observation_control_mismatch')
                p=copy.deepcopy(p);p['forecast']=f;p['planned_bat_angle_deg']=angle_for_position(f['position_world_m'])
            except (ValueError,TypeError,KeyError,OverflowError) as exc:
                statuses[kind,str(exc)]+=1;continue
            counts['valid_histories']+=1;statuses[kind,'valid_history']+=1
            # Exact-observation forecast is a CONTROL origin, not a fitted ability model.
            checks['exact_forecast_position_m']=max(checks['exact_forecast_position_m'],norm(sub(f['position_world_m'],state['input']['ball_position_world_m'])))
            checks['exact_forecast_time_sec']=max(checks['exact_forecast_time_sec'],abs(f['time_sec']-state['input']['reference_time_sec_since_release']))
            for lever in levers:
                base=plan_swing(p,lever,.16);results={}
                for mode in AXES:
                    for error in errors:
                        frame=control_frame(p,base,lever,error,mode)
                        checks['fixed_velocity_ms']=max(checks['fixed_velocity_ms'],norm(sub(frame['velocity'],base['actual_velocity_world_ms'])))
                        checks['vertical_anchor_shift_m']=max(checks['vertical_anchor_shift_m'],norm(sub(sub(frame['point'],base['actual_point_world_m']),(0.,0.,error))))
                        results[mode,error]=trial(state,frame);counts['trial_rows']+=1
                        r=results[mode,error]
                        if r['outgoing_z_ms'] is not None:
                            checks['vertical_velocity_closure_ms']=max(checks['vertical_velocity_closure_ms'],abs(r['outgoing_z_ms']-r['incoming_z_ms']-r['impulse_z_ms']))
                # The axis rules coincide at the zero-error control.
                checks['zero_axis_difference']=max(checks['zero_axis_difference'],norm(sub(
                    control_frame(p,base,lever,0.,AXES[0])['axis'],control_frame(p,base,lever,0.,AXES[1])['axis'])))
                aggregate.add(kind,lever,results,heights)
        aggregate.write(output)
        coverage=dict(first_game_id=first,last_game_id=last,swings=counts['swings'],valid_histories=counts['valid_histories'],
            trials_per_history=2*len(errors)*len(levers),trial_rows=counts['trial_rows'])
        write_csv(output/'symmetric_height_coverage.csv',list(coverage),[coverage])
        write_csv(output/'symmetric_height_status.csv',['adjusted_contact_type','status','states'],
            [dict(adjusted_contact_type=k[0],status=k[1],states=n) for k,n in sorted(statuses.items())])
        write_csv(output/'symmetric_height_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
        (output/'symmetric_height_metadata.json').write_text(json.dumps(dict(version=1,heights_m=list(heights),levers_m=list(levers),
            origin='exact_three_position_forecast_with_saved_execution_angle_error',motion='finite_segment_constant_world_velocity_no_rotation',
            axis_modes=list(AXES),axis_rule='clamp(30-15*((predicted_z-.825)/.325),10,60) then saved execution rotation',
            anchor='baseline_actual_point_plus_world_z_error_in_both_modes',velocity='baseline_plan_actual_reference_velocity_fixed',
            nominal_time='exact_observation_forecast_time_fixed',half_length_m=.16,radius_m=.07,half_window_sec=.06,
            restitution=.5,effective_mass_kg=.6,contact_tolerance_m=1e-6,max_iterations=4000,
            note='Diagnostic geometry control, not live rotating simulation or calibrated contact rate.'),indent=2)+'\n')
        return coverage
    finally:db.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);add_db_argument(p)
    p.add_argument('--first-game-id',type=int,required=True);p.add_argument('--last-game-id',type=int,required=True)
    p.add_argument('--output-dir',type=Path,required=True);p.add_argument('--heights-m',type=float,nargs='+',default=[.01,.03,.05])
    p.add_argument('--levers-m',type=float,nargs='+',default=[.6,.9,1.2]);a=p.parse_args()
    try:print(run(a.db,a.first_game_id,a.last_game_id,a.output_dir,a.heights_m,a.levers_m))
    except (OSError,ValueError,sqlite3.Error) as exc:p.exit(1,f'Diagnostic failed: {exc}\n')
