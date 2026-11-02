#!/usr/bin/env python3
"""Paired execution-frame v2 trials from validated, unchanged saved contact states v1."""
import argparse
import csv
import json
import math
import sqlite3
from collections import Counter, defaultdict
from pathlib import Path

from contact_3d import CONTRACT_VERSION, QualityAccumulator
from contact_collision_probe import cross, dot, write_csv
from contact_state_probe import SOURCE, read_rows, validate_state
from rotating_bat_probe import (FIELDS, SUMMARY_FIELDS, Motion, add, sub, mul, norm,
                                rotate, result_record)

INPUT_SOURCE='shared_pivot_tilt_execution_frame_v2'


def ideal_angle_from_saved_height(state):
    """Exact rule used by calculate_bat_angle for the supported saved-state source."""
    normalized_y=(state['input']['ball_position_world_m'][2]-.825)/.325
    return max(10.,min(60.,30.-15.*normalized_y))


class FrameMotion(Motion):
    """A frame with a shared, translating pivot and its own axis/omega at t_ref."""
    def __init__(self,state,lever,half_length,pivot,pivot_velocity,axis,omega):
        # Initialize ordinary baseline properties, then replace its reference frame.
        super().__init__(state,lever,half_length)
        self.pivot=tuple(pivot);self.pivot_velocity=tuple(pivot_velocity)
        self.axis=tuple(axis);self.omega=tuple(omega)
        self.point=add(self.pivot,mul(self.axis,lever))
        self.velocity=add(self.pivot_velocity,cross(self.omega,mul(self.axis,lever)))
        self.speed_bound=(norm(sub(self.ball_velocity,self.pivot_velocity))
                          +norm(self.omega)*(lever+half_length))


def frames(state,lever,half_length=.16):
    source=state['input'];side=source['handedness_sign']
    ideal_angle=ideal_angle_from_saved_height(state)
    delta_deg=source['bat_angle_deg']-ideal_angle
    tilt=(0.,-side*math.radians(delta_deg),0.)
    a0=(side*math.cos(math.radians(ideal_angle)),0.,math.sin(math.radians(ideal_angle)))
    a1=tuple(state['bat_axis_world']);v1=tuple(state['bat_velocity_world_ms'])
    # Keep the saved actual zero-phase velocity. The ideal orbital velocity is its
    # counterfactual inverse rotation, NOT an independently observed intended velocity.
    vp=mul(a1,dot(v1,a1))
    orbital0=rotate(sub(v1,vp),tilt,-1.)
    omega0=mul(cross(a0,orbital0),1/lever)
    omega1=rotate(omega0,tilt,1.)
    ideal_point=tuple(state['bat_ideal_position_world_m'])
    pivot=sub(ideal_point,mul(a0,lever))
    ideal=FrameMotion(state,lever,half_length,pivot,vp,a0,omega0)
    actual=FrameMotion(state,lever,half_length,pivot,vp,a1,omega1)
    old=Motion(state,lever,half_length)
    record=dict(input_model_version=2,input_source=INPUT_SOURCE,
        ideal_angle_source='calculate_bat_angle_from_saved_height_source_v1',
        ideal_bat_angle_deg=ideal_angle,actual_bat_angle_deg=source['bat_angle_deg'],
        execution_angle_error_deg=delta_deg,handedness_sign=side,lever_m=lever,
        half_length_m=half_length,reference_time_sec_since_release=source['reference_time_sec_since_release'],
        timing_offset_sec=source['timing_offset_sec'],
        ball_position_world_m=source['ball_position_world_m'],ball_velocity_world_ms=source['ball_velocity_world_ms'],
        aim_error_world_m=source['aim_error_world_m'],
        shared_pivot_world_m=pivot,shared_pivot_velocity_world_ms=vp,
        execution_rotation_world_rad=tilt,
        ideal_point_world_m=ideal.point,actual_point_world_m=actual.point,
        ideal_axis_world=ideal.axis,actual_axis_world=actual.axis,
        ideal_velocity_world_ms=ideal.velocity,actual_velocity_world_ms=actual.velocity,
        ideal_omega_world_rad_s=ideal.omega,actual_omega_world_rad_s=actual.omega,
        actual_minus_ideal_position_m=sub(actual.point,ideal.point),
        actual_velocity_saved_residual_ms=norm(sub(actual.velocity,v1)),
        axis_rotation_residual=norm(sub(rotate(a0,tilt,1.),a1)),
        ideal_point_saved_residual_m=norm(sub(ideal.point,ideal_point)),
        shared_pivot_residual_m=norm(sub(ideal.pivot,actual.pivot)))
    checks=dict(actual_velocity_saved_residual_ms=record['actual_velocity_saved_residual_ms'],
        axis_rotation_residual=record['axis_rotation_residual'],
        ideal_point_saved_residual_m=record['ideal_point_saved_residual_m'],
        shared_pivot_residual_m=record['shared_pivot_residual_m'])
    # Independently verify the common-pivot relation at nonzero time and delay too.
    for t in (-.06,0.,.06):
        p0,axis0=ideal.pose(t,ideal.delay);p1,axis1=actual.pose(t,actual.delay)
        p=add(pivot,mul(vp,t-ideal.delay))
        checks['orbit_rotation_residual_m']=max(checks.get('orbit_rotation_residual_m',0),
            norm(sub(sub(p1,p),rotate(sub(p0,p),tilt,1.))))
        checks['axis_time_rotation_residual']=max(checks.get('axis_time_rotation_residual',0),
            norm(sub(axis1,rotate(axis0,tilt,1.))))
        velocity0=add(vp,cross(omega0,mul(axis0,lever)))
        velocity1=add(vp,cross(omega1,mul(axis1,lever)))
        checks['velocity_time_rotation_residual_ms']=max(checks.get('velocity_time_rotation_residual_ms',0),
            norm(sub(sub(velocity1,vp),rotate(sub(velocity0,vp),tilt,1.))))
    if max(checks.values())>1e-8:raise ValueError('execution_frame_identity_mismatch')
    return [('reanchored',old),('ideal_no_execution',ideal),('shared_pivot_execution',actual)],record,checks


def angle_band(degrees):
    x=abs(degrees)
    if x<.1:return '<0.1'
    if x<.5:return '0.1..<0.5'
    if x<1:return '0.5..<1'
    if x<2:return '1..<2'
    return '2+'


class Paired:
    def __init__(self):self.groups=defaultdict(Counter);self.transitions=Counter()

    def add(self,kind,legacy,lever,contrast,before,after):
        key=(kind,legacy,lever,contrast)
        self.transitions[key+(before['status'],after['status'])]+=1
        g=self.groups[key];g['states']+=1
        if not (before['collision_status']==after['collision_status']=='impulse_contact'):return
        g['paired_impulse_contacts']+=1
        for field in ('ev_ms','vla_deg'):
            if before[field] is None or after[field] is None:continue
            g[field+'_n']+=1;g[field+'_before']+=before[field];g[field+'_after']+=after[field]
            g['abs_delta_'+field]+=abs(after[field]-before[field])
        if before['hla_deg'] is not None and after['hla_deg'] is not None:
            dh=(after['hla_deg']-before['hla_deg']+180)%360-180
            g['hla_n']+=1;g['abs_delta_hla_deg']+=abs(dh)
            g['foul_before']+=abs(before['hla_deg'])>=45;g['foul_after']+=abs(after['hla_deg'])>=45
        if before['normal_speed_fraction'] is not None and after['normal_speed_fraction'] is not None:
            g['fraction_n']+=1;g['fraction_before']+=before['normal_speed_fraction'];g['fraction_after']+=after['normal_speed_fraction']

    def write(self,output,prefix="execution_frame"):
        keys=['adjusted_contact_type','legacy_group','lever_m','contrast']
        rows=[]
        for key,g in sorted(self.groups.items()):
            r=dict(zip(keys,key),states=g['states'],paired_impulse_contacts=g['paired_impulse_contacts'])
            for field in ('ev_ms','vla_deg'):
                n=g[field+'_n'];r[field+'_pairs']=n
                for suffix in ('before','after'):r['avg_'+field+'_'+suffix]=g[field+'_'+suffix]/n if n else None
                r['avg_abs_delta_'+field]=g['abs_delta_'+field]/n if n else None
            r.update(hla_pairs=g['hla_n'],avg_abs_delta_hla_deg=g['abs_delta_hla_deg']/g['hla_n'] if g['hla_n'] else None,
                foul_direction_before_pct=100*g['foul_before']/g['hla_n'] if g['hla_n'] else None,
                foul_direction_after_pct=100*g['foul_after']/g['hla_n'] if g['hla_n'] else None,
                normal_fraction_pairs=g['fraction_n'],
                avg_normal_fraction_before=g['fraction_before']/g['fraction_n'] if g['fraction_n'] else None,
                avg_normal_fraction_after=g['fraction_after']/g['fraction_n'] if g['fraction_n'] else None)
            rows.append(r)
        fields=keys+['states','paired_impulse_contacts','ev_ms_pairs','avg_ev_ms_before','avg_ev_ms_after','avg_abs_delta_ev_ms',
            'vla_deg_pairs','avg_vla_deg_before','avg_vla_deg_after','avg_abs_delta_vla_deg','hla_pairs','avg_abs_delta_hla_deg',
            'foul_direction_before_pct','foul_direction_after_pct','normal_fraction_pairs','avg_normal_fraction_before','avg_normal_fraction_after']
        write_csv(output/(prefix+'_pairs.csv'),fields,rows)
        fields=keys+['before_status','after_status']
        write_csv(output/(prefix+'_transitions.csv'),fields+['states'],
            [dict(zip(fields,key),states=n) for key,n in sorted(self.transitions.items())])


class SummaryAccumulator:
    """Keep only aggregate numbers; the 9 trial rows per ball are streamed to CSV."""
    def __init__(self):self.groups=defaultdict(Counter)

    def add(self,row):
        for band in (row['timing_band'],'all'):
            key=(row['adjusted_contact_type'],row['legacy_group'],band,row['variant']);g=self.groups[key]
            g['valid_states']+=1
            for status in ('contact','no_contact','overlap_at_start','unresolved'):
                g[status+'_states']+=row['status']==status
                g['zero_timing_'+status+'_states']+=row['zero_timing_status']==status
            resolved=row['status'] in ('contact','no_contact') and row['zero_timing_status'] in ('contact','no_contact')
            g['resolved_phase_pairs']+=resolved
            g['gained_with_timing']+=resolved and row['status']=='contact' and row['zero_timing_status']=='no_contact'
            g['lost_with_timing']+=resolved and row['status']=='no_contact' and row['zero_timing_status']=='contact'
            for output,field in (('impulse_contacts','ev_ms'),('timed_states','entry_time_offset_ms'),
                    ('defined_hla_contacts','hla_deg'),('defined_vla_contacts','vla_deg'),('paired_contact_times','delta_entry_ms')):
                g[output]+=row[field] is not None
            for output,field in (('avg_entry_time_offset_ms','entry_time_offset_ms'),('avg_delta_entry_ms','delta_entry_ms'),
                    ('avg_ev_ms','ev_ms'),('avg_vla_deg','vla_deg'),('avg_normal_speed_fraction','normal_speed_fraction')):
                if row[field] is not None:g[output]+=row[field];g[output+'_n']+=1
            if row['hla_deg'] is not None:g['foul']+=abs(row['hla_deg'])>=45
            if row['ev_ms'] is not None:g['ev40']+=row['ev_ms']>=40
            for suffix in ('position_error_m','velocity_error_ms','axis_error'):
                g['max_anchor_'+suffix]=max(g['max_anchor_'+suffix],row['anchor_'+suffix])
            g['max_iterations']=max(g['max_iterations'],row['iterations'],row['zero_timing_iterations'])

    def rows(self):
        result=[]
        for key,g in sorted(self.groups.items()):
            row={field:g[field] for field in SUMMARY_FIELDS if field not in ('adjusted_contact_type','legacy_group','timing_band','variant')}
            row.update(zip(('adjusted_contact_type','legacy_group','timing_band','variant'),key))
            row['contact_pct_of_valid']=100*g['contact_states']/g['valid_states']
            for field in ('avg_entry_time_offset_ms','avg_delta_entry_ms','avg_ev_ms','avg_vla_deg','avg_normal_speed_fraction'):
                row[field]=g[field]/g[field+'_n'] if g[field+'_n'] else None
            row['foul_direction_pct']=100*g['foul']/g['defined_hla_contacts'] if g['defined_hla_contacts'] else None
            row['ev_ge_40_pct']=100*g['ev40']/g['impulse_contacts'] if g['impulse_contacts'] else None
            result.append(row)
        return result


EXTRA=['frame_model','execution_angle_error_deg','execution_angle_band','reference_position_shift_m',
       'reference_velocity_vs_saved_ms','input_model_version']


def run(db_path,first,last,output,levers=(.6,.9,1.2),window=.06,half_length=.16,tolerance=1e-6,max_iterations=4000):
    if (first>last or not levers or len(set(levers))!=len(levers) or
        not all(math.isfinite(x) and x>half_length for x in levers) or
        not math.isfinite(window) or window<=0 or not math.isfinite(half_length) or half_length<=0 or
        not math.isfinite(tolerance) or not 0<tolerance<.001 or max_iterations<1):
        raise ValueError('Invalid range or diagnostic parameters')
    output=Path(output)
    db=sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro',uri=True);db.row_factory=sqlite3.Row
    try:
        db.execute('PRAGMA query_only=ON');db.execute('BEGIN');output.mkdir(parents=True,exist_ok=False)
        counts=Counter();statuses=Counter();checks=defaultdict(float);summary=SummaryAccumulator()
        quality=QualityAccumulator();paired=Paired();angle_counts=Counter()
        with (output/'execution_frame_contacts.csv').open('x',newline='',encoding='utf-8') as stream,\
             (output/'execution_frame_inputs.jsonl').open('x',encoding='utf-8') as inputs:
            writer=csv.DictWriter(stream,fieldnames=FIELDS+EXTRA);writer.writeheader()
            for row in read_rows(db,first,last):
                counts['swings']+=1;kind=row['adjusted_contact_type']
                if row['state_version'] is None:reason='missing_state'
                elif row['invalid_reason'] is not None:reason='invalid:'+row['invalid_reason']
                elif row['state_version']!=1 or row['state_source']!=SOURCE:reason='unsupported_saved_state'
                else:
                    try:state=json.loads(row['state_json']);validate_state(state);reason=None
                    except (ValueError,KeyError,TypeError,OverflowError):reason='invalid_saved_payload'
                if reason:statuses[kind,reason]+=1;continue
                # Prepare all radii before emitting any rows, to keep paired denominators equal.
                try:prepared=[frames(state,lever,half_length) for lever in levers]
                except (ValueError,KeyError,TypeError,OverflowError):
                    statuses[kind,'invalid_execution_frame']+=1;continue
                counts['valid_states']+=1;statuses[kind,'valid_state']+=1
                for lever,(trials,input_record,errors) in zip(levers,prepared):
                    input_record.update(game_id=row['game_id'],count_seq=row['count_seq'],adjusted_contact_type=kind)
                    inputs.write(json.dumps(input_record,allow_nan=False)+'\n')
                    for field,value in errors.items():checks[field]=max(checks[field],value)
                    results={};angle=input_record['execution_angle_error_deg']
                    for frame,motion in trials:
                        variant=f'{frame}_r{lever:g}'
                        r=result_record(row,state,motion,variant,max(-window,-state['input']['reference_time_sec_since_release']),
                            window,tolerance,max_iterations,quality)
                        # The inherited infinite-cylinder reference uses the old saved frame.
                        # It is meaningful only for the reanchored control.
                        if frame!='reanchored':
                            r['linear_infinite_status']='not_applicable';r['linear_infinite_entry_offset_ms']=None
                        r.update(frame_model=frame,execution_angle_error_deg=angle,execution_angle_band=angle_band(angle),
                            reference_position_shift_m=norm(sub(motion.point,state['bat_ideal_position_world_m'])),
                            reference_velocity_vs_saved_ms=norm(sub(motion.velocity,state['bat_velocity_world_ms'])),input_model_version=2)
                        writer.writerow(r);results[frame]=r
                        angle_counts[kind,r['legacy_group'],lever,frame,angle_band(angle),r['status']]+=1
                        summary.add(r)
                    legacy=results['reanchored']['legacy_group']
                    paired.add(kind,legacy,lever,'reanchored_to_shared_pivot',results['reanchored'],results['shared_pivot_execution'])
                    paired.add(kind,legacy,lever,'ideal_to_execution',results['ideal_no_execution'],results['shared_pivot_execution'])
        coverage=dict(first_game_id=first,last_game_id=last,swings=counts['swings'],valid_states=counts['valid_states'],
            variants_per_state=3*len(levers),trial_rows=counts['valid_states']*3*len(levers),input_rows=counts['valid_states']*len(levers))
        write_csv(output/'execution_frame_coverage.csv',list(coverage),[coverage])
        write_csv(output/'execution_frame_status.csv',['adjusted_contact_type','status','states'],
            [dict(adjusted_contact_type=k[0],status=k[1],states=n) for k,n in sorted(statuses.items())])
        write_csv(output/'execution_frame_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
        write_csv(output/'execution_frame_summary.csv',SUMMARY_FIELDS+['avg_normal_speed_fraction','ev_ge_40_pct'],summary.rows())
        keys=['adjusted_contact_type','legacy_group','lever_m','frame_model','execution_angle_band','status']
        write_csv(output/'execution_frame_angle_bands.csv',keys+['states'],[dict(zip(keys,k),states=n) for k,n in sorted(angle_counts.items())])
        paired.write(output);quality.write(output)
        (output/'execution_frame_metadata.json').write_text(json.dumps(dict(input_model_version=2,input_source=INPUT_SOURCE,
            contact_contract_version=CONTRACT_VERSION,frames=['reanchored','ideal_no_execution','shared_pivot_execution'],
            solver='conservative_advancement_global_speed_bound',contact_shape='effective_capsule',
            saved_state_version=1,saved_state_source=SOURCE,levers_m=list(levers),half_length_m=half_length,
            effective_radius_m=.07,half_window_sec=window,tolerance_m=tolerance,max_iterations=max_iterations,
            restitution=.5,effective_mass_kg=.6,
            ideal_angle_rule='clamp(30 - 15*((saved_z-.825)/.325),10,60)',
            ideal_velocity_rule='shared_pivot_velocity + inverse_tilt(saved_actual_velocity - shared_pivot_velocity)',
            note='Counterfactual ideal frame derived from supported saved inputs; no new RNG or backfilled DB rows'),indent=2)+'\n')
        return coverage
    finally:db.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--db',type=Path,required=True);p.add_argument('--first-game-id',type=int,required=True)
    p.add_argument('--last-game-id',type=int,required=True);p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--levers-m',type=float,nargs='+',default=[.6,.9,1.2])
    p.add_argument('--half-window-ms',type=float,default=60);p.add_argument('--half-length-m',type=float,default=.16)
    p.add_argument('--contact-tolerance-m',type=float,default=1e-6);p.add_argument('--max-iterations',type=int,default=4000)
    args=p.parse_args()
    try:print(run(args.db,args.first_game_id,args.last_game_id,args.output_dir,args.levers_m,
        args.half_window_ms/1000,args.half_length_m,args.contact_tolerance_m,args.max_iterations))
    except (OSError,ValueError,sqlite3.Error) as exc:p.exit(1,f'Diagnostic failed: {exc}\n')
