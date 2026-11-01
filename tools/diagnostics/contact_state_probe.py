#!/usr/bin/env python3
"""Read saved release-clock contact states; never modify the database."""
import argparse
import json
import math
import sqlite3
from collections import Counter, defaultdict
from pathlib import Path

from contact_collision_probe import (angles, collide, direction, dot, ev_band,
                                     relative_contact_geometry, vla_band, write_csv)

SOURCE = 'plate_constant_accel_linear_bat_world_aim_proxy_v1'


def validate_state(state):
    source = state['input']
    vectors = [source['ball_position_world_m'],source['ball_velocity_world_ms'],source['aim_error_world_m'],
               state['bat_position_world_m'],state['bat_ideal_position_world_m'],state['bat_axis_world'],
               state['bat_velocity_world_ms'],state['bat_angular_velocity_world_rad_s'],
               state['relative_position_world_m'],state['relative_velocity_world_ms']]
    if not all(len(v)==3 and all(isinstance(x,(int,float)) and math.isfinite(x) for x in v) for v in vectors):
        raise ValueError('Invalid saved vector')
    scalar_names=('bat_speed_ms','bat_angle_deg','attack_angle_deg','handedness_sign',
                  'timing_offset_sec','reference_time_sec_since_release','legacy_timing_impact_x_m')
    if not all(isinstance(source[k],(int,float)) and math.isfinite(source[k]) for k in scalar_names):
        raise ValueError('Invalid saved scalar')
    if source['handedness_sign'] not in (-1,1) or source['bat_speed_ms']<0 or abs(source['attack_angle_deg'])>90:
        raise ValueError('Invalid saved bat state')
    if len(source['legacy_contact_offset_xz_m'])!=2 or not all(math.isfinite(v) for v in source['legacy_contact_offset_xz_m']):
        raise ValueError('Invalid saved legacy offset')
    delay = source['timing_offset_sec']
    if not math.isfinite(delay) or not math.isfinite(source['reference_time_sec_since_release']) or source['reference_time_sec_since_release']<=0:
        raise ValueError('Invalid saved clock')
    ball,velocity,aim = vectors[:3]
    bat,ideal,axis,bat_velocity,omega,relative,relative_velocity = vectors[3:]
    reconstructed_r = tuple(b-a for b,a in zip(ball,bat))
    reconstructed_u = tuple(b-a for b,a in zip(velocity,bat_velocity))
    attack=math.radians(source['attack_angle_deg']);theta=math.radians(source['bat_angle_deg'])
    expected_velocity=(0,source['bat_speed_ms']*math.cos(attack),source['bat_speed_ms']*math.sin(attack))
    expected_axis=(source['handedness_sign']*math.cos(theta),0,math.sin(theta))
    checks = dict(bat_velocity_reconstruction_error_ms=max(abs(a-b) for a,b in zip(bat_velocity,expected_velocity)),
        bat_axis_reconstruction_error=max(abs(a-b) for a,b in zip(axis,expected_axis)),
        position_identity_error_m=max(abs(a-b) for a,b in zip(relative,reconstructed_r)),
        phase_identity_error_m=max(abs(relative[i]-aim[i]-bat_velocity[i]*delay) for i in range(3)),
        ideal_position_error_m=max(abs(ideal[i]-ball[i]+aim[i]) for i in range(3)),
        velocity_identity_error_ms=max(abs(a-b) for a,b in zip(relative_velocity,reconstructed_u)),
        axis_length_error=abs(math.sqrt(dot(axis,axis))-1),
        angular_velocity_max_abs=max(abs(v) for v in omega))
    if max(checks.values())>1e-8:
        raise ValueError('Saved-state identity mismatch')
    return reconstructed_r,checks


def run(db_path, first, last, output):
    if first>last:
        raise ValueError('Invalid game range')
    db = sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro',uri=True)
    db.row_factory=sqlite3.Row
    try:
        db.execute('PRAGMA query_only=ON')
        db.execute('BEGIN')
        rows=db.execute('''WITH keys AS (
            SELECT game_id,count_seq FROM player_game_batting
            WHERE game_id BETWEEN ? AND ? AND adjusted_contact_type IN
              ('SwungAndMiss','SolidContact','WeakContact','MarginalContact')
            UNION SELECT game_id,count_seq FROM player_game_contact_state WHERE game_id BETWEEN ? AND ?
            ) SELECT k.game_id,k.count_seq,COALESCE(b.adjusted_contact_type,'UnrecordedBatting') AS adjusted_contact_type,
            b.result,b.launch_speed,b.launch_angle,b.final_hla_deg,
            s.state_version,s.state_source,s.invalid_reason,s.state_json
            FROM keys k LEFT JOIN player_game_batting b ON b.game_id=k.game_id AND b.count_seq=k.count_seq
            LEFT JOIN player_game_contact_state s ON s.game_id=k.game_id AND s.count_seq=k.count_seq
            ORDER BY k.game_id,k.count_seq''',(first,last,first,last))
        output=Path(output)
        output.mkdir(parents=True,exist_ok=False)
        coverage=Counter();statuses=Counter();checks=defaultdict(float)
        summary=defaultdict(Counter);dist=Counter();details=[]
        for row in rows:
            coverage['swings']+=1
            kind=row['adjusted_contact_type']
            coverage['unrecorded_batting_swings']+=kind=='UnrecordedBatting'
            if row['state_version'] is None:
                statuses[kind,'missing_state']+=1
                continue
            coverage['recorded_swings']+=1
            if row['invalid_reason'] is not None:
                statuses[kind,'invalid:'+row['invalid_reason']]+=1
                continue
            if row['state_version']!=1 or row['state_source']!=SOURCE:
                statuses[kind,'unsupported_state_version_or_source']+=1
                continue
            try:
                state=json.loads(row['state_json'])
                relative,errors=validate_state(state)
            except (TypeError,KeyError,ValueError,OverflowError):
                statuses[kind,'invalid_saved_payload']+=1
                continue
            coverage['valid_states']+=1
            for key,value in errors.items():
                checks[key]=max(checks[key],value)
            source=state['input'];axis=state['bat_axis_world'];bat=state['bat_velocity_world_ms']
            entry,reason=relative_contact_geometry(source['ball_velocity_world_ms'],bat,axis,relative)
            if reason:
                statuses[kind,reason]+=1
                continue
            coverage['cylinder_intersections']+=1
            statuses[kind,'cylinder_intersection']+=1
            dt=entry['first_contact_time_offset_sec']
            r_contact=tuple(r+u*dt for r,u in zip(relative,state['relative_velocity_world_ms']))
            axial=dot(r_contact,axis)
            checks['max_abs_contact_time_offset_sec']=max(checks['max_abs_contact_time_offset_sec'],abs(dt))
            checks['max_radius_error_m']=max(checks['max_radius_error_m'],entry['contact_radius_error_m'])
            details.append(dict(game_id=row['game_id'],count_seq=row['count_seq'],adjusted_contact_type=kind,
                timing_offset_ms=source['timing_offset_sec']*1000,
                legacy_x_m=source['legacy_contact_offset_xz_m'][0],new_relative_x_m=relative[0],
                new_relative_y_m=relative[1],new_relative_z_m=relative[2],
                b_over_r=entry['impact_parameter_m']/.07,time_offset_sec=dt,
                time_since_release_sec=source['reference_time_sec_since_release']+dt,
                axial_at_entry_m=axial,within_legacy_axial_window=int(abs(axial)<=.16)))
            options=[]
            if kind in ('SolidContact','WeakContact','MarginalContact'):
                options.append(('current',row['launch_speed'],row['final_hla_deg'],row['launch_angle']))
            for e in (.3,.5,.7):
                for mass in (.3,.6):
                    outgoing,_,_=collide(source['ball_velocity_world_ms'],bat,entry['normal'],e,mass)
                    options.append((f'shadow_e{e:g}_m{mass:g}',*angles(outgoing)))
            for variant,ev,hla,vla in options:
                g=summary[kind,variant];g['contacts']+=1;g['ev']+=ev
                g['within_axial']+=abs(axial)<=.16
                if hla is not None:
                    g['hla_n']+=1;g['foul']+=abs(hla)>=45;g['backward']+=abs(hla)>=90
                if vla is not None:
                    g['vla_n']+=1;g['vla']+=vla;g['abs_vla']+=abs(vla)
                    g['negative_vla']+=vla<0;g['line_vla']+=10<=vla<25
                dist[kind,variant,ev_band(ev),vla_band(vla),direction(hla)]+=1
        cover=dict(first_game_id=first,last_game_id=last,swings=coverage['swings'],
                   recorded_swings=coverage['recorded_swings'],unrecorded_batting_swings=coverage['unrecorded_batting_swings'],
                   valid_states=coverage['valid_states'],
                   cylinder_intersections=coverage['cylinder_intersections'])
        write_csv(output/'state_coverage.csv',list(cover),[cover])
        write_csv(output/'state_status.csv',['adjusted_contact_type','status','swings'],
                  [dict(adjusted_contact_type=k[0],status=k[1],swings=n) for k,n in sorted(statuses.items())])
        write_csv(output/'state_checks.csv',['metric','max_error_or_value'],
                  [dict(metric=k,max_error_or_value=v) for k,v in sorted(checks.items())])
        stats=[]
        for (kind,variant),g in sorted(summary.items()):
            stats.append(dict(adjusted_contact_type=kind,variant=variant,contacts=g['contacts'],
                avg_ev_ms=g['ev']/g['contacts'],avg_vla_deg=g['vla']/g['vla_n'] if g['vla_n'] else None,
                avg_abs_vla_deg=g['abs_vla']/g['vla_n'] if g['vla_n'] else None,
                defined_hla_contacts=g['hla_n'],defined_vla_contacts=g['vla_n'],
                foul_direction_pct=100*g['foul']/g['hla_n'] if g['hla_n'] else None,
                backward_direction_pct=100*g['backward']/g['hla_n'] if g['hla_n'] else None,
                negative_vla_pct=100*g['negative_vla']/g['vla_n'] if g['vla_n'] else None,
                vla_10_25_pct=100*g['line_vla']/g['vla_n'] if g['vla_n'] else None,
                within_legacy_axial_window_pct=100*g['within_axial']/g['contacts']))
        fields=['adjusted_contact_type','variant','contacts','avg_ev_ms','avg_vla_deg','avg_abs_vla_deg',
                'defined_hla_contacts','defined_vla_contacts','foul_direction_pct','backward_direction_pct',
                'negative_vla_pct','vla_10_25_pct','within_legacy_axial_window_pct']
        write_csv(output/'state_collision_summary.csv',fields,stats)
        fields=['adjusted_contact_type','variant','ev_band','vla_band','launch_direction']
        write_csv(output/'state_collision_distribution.csv',fields+['contacts'],
                  [dict(zip(fields,k),contacts=n) for k,n in sorted(dist.items())])
        fields=['game_id','count_seq','adjusted_contact_type','timing_offset_ms','legacy_x_m','new_relative_x_m',
                'new_relative_y_m','new_relative_z_m','b_over_r','time_offset_sec','time_since_release_sec',
                'axial_at_entry_m','within_legacy_axial_window']
        write_csv(output/'state_contacts.csv',fields,details)
        return cover
    finally:
        db.close()


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--db',type=Path,required=True)
    parser.add_argument('--first-game-id',type=int,required=True)
    parser.add_argument('--last-game-id',type=int,required=True)
    parser.add_argument('--output-dir',type=Path,required=True)
    args=parser.parse_args()
    try:
        print(run(args.db,args.first_game_id,args.last_game_id,args.output_dir))
    except (OSError,ValueError,sqlite3.Error) as exc:
        parser.exit(1,f'Diagnostic failed: {exc}\n')
