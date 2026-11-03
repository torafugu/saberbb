#!/usr/bin/env python3
"""Read-only paired finite-segment linear/rotating bat trajectory trials (state v1)."""
import argparse
from diagnostic_db import add_db_argument
import csv
import json
import math
import sqlite3
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

from contact_collision_probe import cross, dot, relative_contact_geometry, write_csv
from contact_3d import CONTRACT_VERSION, ContactGeometry, QualityAccumulator, solve_contact
from contact_state_miss import legacy_group, timing_band
from contact_state_probe import SOURCE, read_rows, validate_state


def add(a,b): return tuple(x+y for x,y in zip(a,b))
def sub(a,b): return tuple(x-y for x,y in zip(a,b))
def mul(a,s): return tuple(x*s for x in a)
def norm(a): return math.sqrt(dot(a,a))


def rotate(a, omega, t):
    speed=norm(omega)
    if speed==0: return tuple(a)
    k=mul(omega,1/speed);phase=speed*t
    return add(add(mul(a,math.cos(phase)),mul(cross(k,a),math.sin(phase))),
               mul(k,dot(k,a)*(1-math.cos(phase))))


@dataclass
class Motion:
    """An explicit hypothesis anchored to the ideal saved reference point and speed."""
    state: dict
    lever: float | None
    half_length: float = .16
    radius: float = .07

    def __post_init__(self):
        source=self.state['input']
        self.ball=tuple(source['ball_position_world_m'])
        self.ball_velocity=tuple(source['ball_velocity_world_ms'])
        self.point=tuple(self.state['bat_ideal_position_world_m'])
        self.axis=tuple(self.state['bat_axis_world'])
        self.velocity=tuple(self.state['bat_velocity_world_ms'])
        self.delay=source['timing_offset_sec']
        if self.lever is None:
            self.omega=(0,0,0);self.pivot_velocity=self.velocity;self.pivot=self.point
            self.speed_bound=norm(sub(self.ball_velocity,self.velocity))
        else:
            self.omega=mul(cross(self.axis,self.velocity),1/self.lever)
            self.pivot=sub(self.point,mul(self.axis,self.lever))
            # The component parallel to the axis requires pivot translation.
            self.pivot_velocity=mul(self.axis,dot(self.velocity,self.axis))
            self.speed_bound=(norm(sub(self.ball_velocity,self.pivot_velocity))
                              +norm(self.omega)*(self.lever+self.half_length))

    def pose(self,t,delay):
        phase_time=t-delay
        axis=rotate(self.axis,self.omega,phase_time)
        if self.lever is None:
            point=add(self.point,mul(self.velocity,phase_time))
        else:
            point=add(add(self.pivot,mul(self.pivot_velocity,phase_time)),mul(axis,self.lever))
        return point,axis

    def evaluate(self,t,delay):
        point,axis=self.pose(t,delay)
        ball=add(self.ball,mul(self.ball_velocity,t))
        displacement=sub(ball,point)
        axial=dot(displacement,axis)
        clamped=max(-self.half_length,min(self.half_length,axial))
        closest=add(point,mul(axis,clamped))
        radial=sub(ball,closest);distance=norm(radial)
        velocity=(self.velocity if self.lever is None else
                  add(self.pivot_velocity,cross(self.omega,mul(axis,self.lever+clamped))))
        return dict(clearance_m=distance-self.radius,axial_m=axial,
            closest_axial_m=clamped,normal=mul(radial,1/distance) if distance>0 else None,
            bat_velocity=velocity,axis=axis,
            contact_region='side' if abs(axial)<=self.half_length else 'cap')


def search(motion,delay,start,end,tolerance=1e-6,max_iterations=4000):
    """Conservative advancement using a global bound on relative segment speed.

    Outside clearance / speed_bound is a safe time increment toward contact.
    Contact is reported within a spatial tolerance; stalled/grazing searches
    exceeding the budget remain unresolved rather than being counted as misses.
    """
    t=start;minimum=math.inf
    for iteration in range(1,max_iterations+1):
        value=motion.evaluate(t,delay)
        gap=value['clearance_m'];minimum=min(minimum,gap)
        if gap<=tolerance:
            return dict(status='overlap_at_start' if iteration==1 else 'contact',
                time_offset_sec=t,iterations=iteration,min_evaluated_clearance_m=minimum,**value)
        if t>=end or motion.speed_bound==0:
            return dict(status='no_contact',time_offset_sec=None,iterations=iteration,
                min_evaluated_clearance_m=minimum)
        safe_step=.9*gap/motion.speed_bound
        next_time=min(end,t+safe_step)
        if next_time<=t:
            break
        t=next_time
    return dict(status='unresolved',time_offset_sec=None,iterations=iteration,
        min_evaluated_clearance_m=minimum)


def check_anchor(motion):
    point,axis=motion.pose(0,0)
    velocity=(motion.velocity if motion.lever is None else
              add(motion.pivot_velocity,cross(motion.omega,mul(axis,motion.lever))))
    return dict(anchor_position_error_m=norm(sub(point,motion.point)),
        anchor_velocity_error_ms=norm(sub(velocity,motion.velocity)),
        anchor_axis_error=norm(sub(axis,motion.axis)))


FIELDS=['game_id','count_seq','adjusted_contact_type','legacy_group','timing_band','timing_offset_ms',
    'variant','lever_m','half_length_m','effective_radius_m','window_start_ms','window_end_ms',
    'status','zero_timing_status','entry_time_offset_ms','entry_since_release_sec',
    'zero_timing_entry_offset_ms','delta_entry_ms','axial_at_entry_m','contact_region',
    'axis_at_entry_x','axis_at_entry_y','axis_at_entry_z','rotation_angle_at_entry_deg',
    'normal_x','normal_y','normal_z','point_velocity_x_ms','point_velocity_y_ms','point_velocity_z_ms',
    'relative_normal_ms','impulse_delta_ms','ev_ms','hla_deg','vla_deg',
    'iterations','zero_timing_iterations','min_evaluated_clearance_m',
    'phase_position_difference_m','phase_velocity_difference_ms',
    'omega_rad_s','omega_x_rad_s','omega_y_rad_s','omega_z_rad_s','anchor_position_error_m','anchor_velocity_error_ms','anchor_axis_error',
    'linear_infinite_status','linear_infinite_entry_offset_ms',
    'contact_contract_version','collision_status','collision_invalid_reason','closest_axial_at_entry_m',
    'ball_velocity_x_ms','ball_velocity_y_ms','ball_velocity_z_ms',
    'outgoing_velocity_x_ms','outgoing_velocity_y_ms','outgoing_velocity_z_ms',
    'normal_impulse_kg_ms','relative_speed_ms','approach_speed_ms','normal_speed_fraction','axial_fraction',
    'normal_elevation_deg','bat_point_speed_ms','restitution_residual_ms','tangential_impulse_residual_ms']


def result_record(row,state,motion,variant,start,end,tolerance,max_iterations,quality=None):
    source=state['input'];delay=source['timing_offset_sec']
    actual=search(motion,delay,start,end,tolerance,max_iterations)
    zero=search(motion,0,start,end,tolerance,max_iterations)
    infinite,reason=relative_contact_geometry(source['ball_velocity_world_ms'],motion.velocity,
        motion.axis,state['relative_position_world_m'])
    r={key:None for key in FIELDS}
    r.update(game_id=row['game_id'],count_seq=row['count_seq'],adjusted_contact_type=row['adjusted_contact_type'],
        legacy_group=legacy_group(source)[0],timing_band=timing_band(delay),timing_offset_ms=delay*1000,
        variant=variant,lever_m=motion.lever,half_length_m=motion.half_length,effective_radius_m=motion.radius,
        window_start_ms=start*1000,window_end_ms=end*1000,status=actual['status'],zero_timing_status=zero['status'],
        iterations=actual['iterations'],zero_timing_iterations=zero['iterations'],
        min_evaluated_clearance_m=actual['min_evaluated_clearance_m'],omega_rad_s=norm(motion.omega),
        linear_infinite_status=reason or 'cylinder_intersection',
        linear_infinite_entry_offset_ms=infinite['first_contact_time_offset_sec']*1000 if infinite else None,
        **check_anchor(motion))
    phase_point,phase_axis=motion.pose(0,delay)
    phase_velocity=(motion.velocity if motion.lever is None else
        add(motion.pivot_velocity,cross(motion.omega,mul(phase_axis,motion.lever))))
    for axis,value in zip('xyz',motion.omega): r['omega_'+axis+'_rad_s']=value
    r['phase_position_difference_m']=norm(sub(phase_point,state['bat_position_world_m']))
    r['phase_velocity_difference_ms']=norm(sub(phase_velocity,motion.velocity))
    if actual['time_offset_sec'] is not None:
        r.update(entry_time_offset_ms=actual['time_offset_sec']*1000,
            entry_since_release_sec=source['reference_time_sec_since_release']+actual['time_offset_sec'],
            axial_at_entry_m=actual['axial_m'],contact_region=actual['contact_region'],
            rotation_angle_at_entry_deg=math.degrees(norm(motion.omega)*(actual['time_offset_sec']-delay)))
        for axis,value in zip('xyz',actual['axis']): r['axis_at_entry_'+axis]=value
    if zero['time_offset_sec'] is not None:
        r['zero_timing_entry_offset_ms']=zero['time_offset_sec']*1000
    if actual['status']=='contact' and zero['status']=='contact':
        r['delta_entry_ms']=(actual['time_offset_sec']-zero['time_offset_sec'])*1000
    metrics=None
    r['contact_contract_version']=CONTRACT_VERSION
    r['collision_status']='not_evaluated:'+actual['status']
    # Initial overlaps lack a resolved first-contact normal and are not used for EV/VLA.
    if actual['status']=='contact' and actual['normal'] is not None:
        for axis,value in zip('xyz',actual['normal']): r['normal_'+axis]=value
        for axis,value in zip('xyz',actual['bat_velocity']): r['point_velocity_'+axis+'_ms']=value
        r['closest_axial_at_entry_m']=actual['closest_axial_m']
        for axis,value in zip('xyz',motion.ball_velocity): r['ball_velocity_'+axis+'_ms']=value
        geometry=ContactGeometry(r['entry_since_release_sec'],tuple(actual['axis']),tuple(actual['normal']),
            actual['axial_m'],actual['closest_axial_m'],motion.half_length,actual['contact_region'])
        try:
            metrics=solve_contact(geometry,motion.ball_velocity,actual['bat_velocity'])
        except ValueError as exc:
            r['collision_status']='invalid_contact';r['collision_invalid_reason']=str(exc)
            metrics={'collision_status':'invalid_contact'}
        else:
            for field,value in metrics.items():
                if field in FIELDS:r[field]=value
            if metrics['outgoing_world_ms'] is not None:
                for axis,value in zip('xyz',metrics['outgoing_world_ms']): r['outgoing_velocity_'+axis+'_ms']=value
    if quality is not None:quality.add(r,metrics)
    return r


def summarize(groups):
    result=[]
    for key,rows in sorted(groups.items()):
        r=dict(zip(('adjusted_contact_type','legacy_group','timing_band','variant'),key))
        def avg(field):
            values=[x[field] for x in rows if x[field] is not None]
            return sum(values)/len(values) if values else None
        for status in ('contact','no_contact','overlap_at_start','unresolved'):
            r[status+'_states']=sum(x['status']==status for x in rows)
            r['zero_timing_'+status+'_states']=sum(x['zero_timing_status']==status for x in rows)
        paired=[x for x in rows if x['status'] in ('contact','no_contact') and x['zero_timing_status'] in ('contact','no_contact')]
        r.update(valid_states=len(rows),contact_pct_of_valid=100*r['contact_states']/len(rows),
            resolved_phase_pairs=len(paired),
            gained_with_timing=sum(x['status']=='contact' and x['zero_timing_status']=='no_contact' for x in paired),
            lost_with_timing=sum(x['status']=='no_contact' and x['zero_timing_status']=='contact' for x in paired),
            impulse_contacts=sum(x['ev_ms'] is not None for x in rows),
            timed_states=sum(x['entry_time_offset_ms'] is not None for x in rows),
            defined_hla_contacts=sum(x['hla_deg'] is not None for x in rows),
            defined_vla_contacts=sum(x['vla_deg'] is not None for x in rows),
            paired_contact_times=sum(x['delta_entry_ms'] is not None for x in rows),
            avg_entry_time_offset_ms=avg('entry_time_offset_ms'),avg_delta_entry_ms=avg('delta_entry_ms'),
            avg_ev_ms=avg('ev_ms'),avg_vla_deg=avg('vla_deg'),
            foul_direction_pct=(100*sum(abs(x['hla_deg'])>=45 for x in rows if x['hla_deg'] is not None)
                /sum(x['hla_deg'] is not None for x in rows)) if any(x['hla_deg'] is not None for x in rows) else None,
            max_anchor_position_error_m=max(x['anchor_position_error_m'] for x in rows),
            max_anchor_velocity_error_ms=max(x['anchor_velocity_error_ms'] for x in rows),
            max_anchor_axis_error=max(x['anchor_axis_error'] for x in rows),
            max_iterations=max(max(x['iterations'],x['zero_timing_iterations']) for x in rows))
        result.append(r)
    return result


SUMMARY_FIELDS=['adjusted_contact_type','legacy_group','timing_band','variant']+[
    prefix+status+'_states' for status in ('contact','no_contact','overlap_at_start','unresolved') for prefix in ('','zero_timing_')]+[
    'valid_states','contact_pct_of_valid','resolved_phase_pairs','gained_with_timing','lost_with_timing',
    'impulse_contacts','timed_states','defined_hla_contacts','defined_vla_contacts','paired_contact_times','avg_entry_time_offset_ms','avg_delta_entry_ms',
    'avg_ev_ms','avg_vla_deg','foul_direction_pct','max_anchor_position_error_m',
    'max_anchor_velocity_error_ms','max_anchor_axis_error','max_iterations']


def run(db_path,first,last,output,levers=(.6,.9,1.2),window=.06,half_length=.16,
        tolerance=1e-6,max_iterations=4000):
    if (first>last or not levers or len(set(levers))!=len(levers) or
        not all(math.isfinite(x) and x>half_length for x in levers) or
        not math.isfinite(window) or window<=0 or not math.isfinite(half_length) or half_length<=0 or
        not math.isfinite(tolerance) or not 0<tolerance<.001 or max_iterations<1):
        raise ValueError('Invalid game range, geometry or solver parameters')
    output=Path(output)
    db=sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro',uri=True);db.row_factory=sqlite3.Row
    try:
        db.execute('PRAGMA query_only=ON');db.execute('BEGIN')
        output.mkdir(parents=True,exist_ok=False)
        counts=Counter();statuses=Counter();groups=defaultdict(list);transitions=Counter();quality=QualityAccumulator()
        with (output/'rotating_contacts.csv').open('x',newline='',encoding='utf-8') as stream:
            writer=csv.DictWriter(stream,fieldnames=FIELDS);writer.writeheader()
            for row in read_rows(db,first,last):
                counts['swings']+=1;kind=row['adjusted_contact_type']
                if row['state_version'] is None: reason='missing_state'
                elif row['invalid_reason'] is not None: reason='invalid:'+row['invalid_reason']
                elif row['state_version']!=1 or row['state_source']!=SOURCE: reason='unsupported_state_version_or_source'
                else:
                    try:
                        state=json.loads(row['state_json']);validate_state(state);reason=None
                    except (TypeError,KeyError,ValueError,OverflowError): reason='invalid_saved_payload'
                if reason:
                    statuses[kind,reason]+=1;continue
                statuses[kind,'valid_state']+=1;counts['valid_states']+=1
                # The local ball approximation must not extend before release.
                start=max(-window,-state['input']['reference_time_sec_since_release']);end=window
                trials=[('linear_finite',None)]+[(f'rotating_r{lever:g}',lever) for lever in levers]
                baseline=None
                for variant,lever in trials:
                    motion=Motion(state,lever,half_length)
                    record=result_record(row,state,motion,variant,start,end,tolerance,max_iterations,quality)
                    writer.writerow(record)
                    for band in (record['timing_band'],'all'):
                        groups[kind,record['legacy_group'],band,variant].append(record)
                    if lever is None: baseline=record['status']
                    else: transitions[kind,record['legacy_group'],variant,baseline,record['status']]+=1
        coverage=dict(first_game_id=first,last_game_id=last,swings=counts['swings'],valid_states=counts['valid_states'],
            variants_per_state=1+len(levers),trial_rows=counts['valid_states']*(1+len(levers)))
        write_csv(output/'rotating_coverage.csv',list(coverage),[coverage])
        write_csv(output/'rotating_state_status.csv',['adjusted_contact_type','status','swings'],
            [dict(adjusted_contact_type=k[0],status=k[1],swings=v) for k,v in sorted(statuses.items())])
        write_csv(output/'rotating_summary.csv',SUMMARY_FIELDS,summarize(groups))
        fields=['adjusted_contact_type','legacy_group','variant','linear_status','rotating_status']
        write_csv(output/'rotating_transitions.csv',fields+['swings'],
            [dict(zip(fields,k),swings=v) for k,v in sorted(transitions.items())])
        quality.write(output)
        (output/'rotating_metadata.json').write_text(json.dumps(dict(probe_version=2,contact_contract_version=CONTRACT_VERSION,state_source=SOURCE,
            levers_m=list(levers),half_length_m=half_length,effective_radius_m=.07,
            half_window_sec=window,contact_tolerance_m=tolerance,max_iterations=max_iterations,
            restitution=.5,effective_mass_kg=.6,solver='conservative_advancement_global_speed_bound',
            ball_motion='constant_saved_plate_velocity',
            pivot_motion='constant_axis_parallel_translation',contact_shape='effective_capsule',
            note='Hypothesis trials, not replayed game outcomes or calibrated physical parameters'),indent=2)+'\n')
        return coverage
    finally: db.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    add_db_argument(p);p.add_argument('--first-game-id',type=int,required=True)
    p.add_argument('--last-game-id',type=int,required=True);p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--levers-m',type=float,nargs='+',default=[.6,.9,1.2])
    p.add_argument('--half-window-ms',type=float,default=60)
    p.add_argument('--half-length-m',type=float,default=.16)
    p.add_argument('--contact-tolerance-m',type=float,default=1e-6)
    p.add_argument('--max-iterations',type=int,default=4000)
    args=p.parse_args()
    try: print(run(args.db,args.first_game_id,args.last_game_id,args.output_dir,args.levers_m,
        args.half_window_ms/1000,args.half_length_m,args.contact_tolerance_m,args.max_iterations))
    except (OSError,ValueError,sqlite3.Error) as exc: p.exit(1,f'Diagnostic failed: {exc}\n')
