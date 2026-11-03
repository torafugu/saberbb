#!/usr/bin/env python3
"""Paired signed-tilt trial with matched saved anchor position, velocity and clock."""
import argparse
import csv
import json
import math
import sqlite3
from collections import Counter, defaultdict
from pathlib import Path

from bat_axis_origin_audit import run as audit_source, tilt, check
from contact_3d import ContactGeometry, solve_contact, band
from contact_collision_probe import cross, dot, write_csv
from contact_state_probe import SOURCE, validate_state
from execution_frame_probe import FrameMotion
from effective_mass_probe import direction
from finite_barrel_probe import configs, event
from forecast_contact_geometry import audit, vec
from pitch_spin_source import PitchSpinSource
from rotating_bat_probe import add, sub, mul, norm, rotate, search

MODEL_NAMES = ('normal_profile', 'finite_mu0', 'finite_I0.00015_et0.4')
METRICS = ('ev_ms','vla_deg','abs_normal_x','normal_z','entry_tilt_deg','proxy_pivot_z_m',
           'point_speed_ms','abs_outgoing_x_ms','outgoing_y_ms')
KEYS = ['scope','variant','stratum','band','orientation','model']


def counterfactual(plan):
    """Rotate anchor axis/omega about Y; compensate pivot to preserve actual anchor P,V."""
    a = plan['actual_axis_world']; h = 1 if plan['ideal_axis_world'][0] > 0 else -1
    theta = math.atan2(a[2],h*a[0]); rotation = (0.,2*h*theta,0.)
    axis = rotate(a,rotation,1.)
    omega = rotate(plan['actual_omega_world_rad_s'],rotation,1.)
    point = plan['actual_point_world_m']; velocity = plan['actual_velocity_world_ms']
    pivot = sub(point,mul(axis,plan['lever_m']))
    vp = sub(velocity,cross(omega,mul(axis,plan['lever_m'])))
    return dict(plan,pivot_world_m=pivot,pivot_velocity_world_ms=vp,
        actual_axis_world=axis,actual_omega_world_rad_s=omega)


def motion_for(state,plan):
    return FrameMotion(state,plan['lever_m'],plan['half_length_m'],plan['pivot_world_m'],
        plan['pivot_velocity_world_ms'],plan['actual_axis_world'],plan['actual_omega_world_rad_s'])


def evaluate(row,state,plan,checks):
    """One timed search; no zero-delay replay or ground calculation."""
    m = motion_for(state,plan)
    delay = plan['nominal_time_sec']-state['input']['reference_time_sec_since_release']
    actual = search(m,delay,row['window_start_ms']/1000,row['window_end_ms']/1000)
    r = dict(row,status=actual['status'],collision_status='not_evaluated:'+actual['status'],
        contact_region=None,ev_ms=None,hla_deg=None,vla_deg=None,closest_axial_at_entry_m=None,
        min_evaluated_clearance_m=actual['min_evaluated_clearance_m'])
    for k in ('entry_since_release_sec','entry_time_offset_ms','axial_at_entry_m'):
        r[k]=None
    if actual['status'] != 'contact': return r
    t = actual['time_offset_sec']
    r.update(entry_since_release_sec=state['input']['reference_time_sec_since_release']+t,
        entry_time_offset_ms=t*1000,axial_at_entry_m=actual['axial_m'],
        closest_axial_at_entry_m=actual['closest_axial_m'],contact_region=actual['contact_region'])
    for prefix,values in [('axis_at_entry_',actual['axis']),('normal_',actual['normal']),
        ('point_velocity_',actual['bat_velocity']),('ball_velocity_',m.ball_velocity)]:
        for a,v in zip('xyz',values):r[prefix+a+('_ms' if 'velocity' in prefix else '')]=v
    for a,v in zip('xyz',m.omega):r['omega_'+a+'_rad_s']=v
    g = ContactGeometry(r['entry_since_release_sec'],actual['axis'],actual['normal'],
        actual['axial_m'],actual['closest_axial_m'],m.half_length,actual['contact_region'])
    result = solve_contact(g,m.ball_velocity,actual['bat_velocity'])
    for key,value in result.items():
        if key != 'outgoing_world_ms': r[key]=value
    if result['outgoing_world_ms'] is not None:
        for a,v in zip('xyz',result['outgoing_world_ms']):r['outgoing_velocity_'+a+'_ms']=v
    return r


def observations(r,plan,result):
    values = dict(proxy_pivot_z_m=plan['pivot_world_m'][2])
    if r['status']=='contact':
        values.update(abs_normal_x=abs(float(r['normal_x'])),normal_z=float(r['normal_z']),
            entry_tilt_deg=tilt(vec(r,'axis_at_entry_')),point_speed_ms=norm(vec(r,'point_velocity_')))
    for k in ('ev_ms','vla_deg'):
        if result.get(k) is not None:values[k]=result[k]
    if result.get('outgoing_world_ms') is not None:
        v=result['outgoing_world_ms'];values.update(abs_outgoing_x_ms=abs(v[0]),outgoing_y_ms=v[1])
    return values


class Aggregate:
    def __init__(self):
        self.summary=defaultdict(Counter);self.pairs=defaultdict(Counter);self.transitions=Counter()
        self.histogram=Counter();self.histogram_totals=Counter()
    def add(self,row,plans,rows,results):
        for scope in ('all_swings','recorded_batting_only'):
            if scope=='recorded_batting_only' and row['adjusted_contact_type']=='UnrecordedBatting':continue
            theta=tilt(plans['saved_up']['ideal_axis_world'])
            for stratum,b in [('overall','all'),('planned_tilt',band(theta,(10,20,30,40,50,60,75,90)))]:
                for model in MODEL_NAMES:
                    key=(scope,row['variant'],stratum,b)
                    for orient in plans:
                        r=rows[orient];result=results[orient][model];g=self.summary[key+(orient,model)]
                        g['trials']+=1;g['geometry_contacts']+=r['status']=='contact'
                        g['successful_contacts']+=result['collision_status']=='impulse_contact'
                        g['fair_contacts']+=direction(result)=='fair_direction'
                        g['proxy_pivot_below_ground']+=plans[orient]['pivot_world_m'][2]<0
                        for metric,value in observations(r,plans[orient],result).items():
                            g[metric]+=value;g['n_'+metric]+=1
                        if result.get('ev_ms') is not None:
                            g['low_ev']+=result['ev_ms']<25;g['high_ev']+=result['ev_ms']>=45
                            g['downward']+=result['vla_deg']<0;g['high_vla']+=result['vla_deg']>=50
                            if stratum=='overall':
                                for cohort in ('all_successful','fair_direction'):
                                    if cohort=='fair_direction' and direction(result)!=cohort:continue
                                    hk=(scope,row['variant'],orient,model,cohort)
                                    self.histogram_totals[hk]+=1
                                    self.histogram[hk+(band(result['ev_ms'],(0,10,20,25,30,35,40,45,50,55,60)),
                                        band(result['vla_deg'],(-90,-60,-30,0,10,20,30,40,50,60,90)))]+=1
                    l,r=(results[o][model] for o in ('saved_up','matched_down'))
                    self.transitions[key+(model,rows['saved_up']['status'],rows['matched_down']['status'],
                        l['collision_status'],r['collision_status'],direction(l),direction(r))]+=1
                    p=self.pairs[key+(model,)];p['trials']+=1
                    if l['collision_status']==r['collision_status']=='impulse_contact':
                        p['paired_successful_contacts']+=1
                        p['before_fair']+=direction(l)=='fair_direction';p['after_fair']+=direction(r)=='fair_direction'
                        for k in ('ev_ms','vla_deg'):
                            p['before_'+k]+=l[k];p['after_'+k]+=r[k]
    def write(self,out):
        rows=[]
        counts=['trials','geometry_contacts','successful_contacts','fair_contacts','proxy_pivot_below_ground','low_ev','high_ev','downward','high_vla']
        for key,g in sorted(self.summary.items()):
            r=dict(zip(KEYS,key));r.update({k:g[k] for k in counts})
            for k in METRICS:r['n_'+k]=g['n_'+k];r['avg_'+k]=g[k]/g['n_'+k] if g['n_'+k] else None
            r['contact_fraction']=g['geometry_contacts']/g['trials']
            r['fair_fraction_of_successful']=g['fair_contacts']/g['successful_contacts'] if g['successful_contacts'] else None
            rows.append(r)
        write_csv(out/'bat_pivot_orientation_summary.csv',KEYS+counts+['contact_fraction','fair_fraction_of_successful']+[p+k for k in METRICS for p in ('n_','avg_')],rows)
        keys=KEYS[:4]+['model'];rows=[]
        for key,g in sorted(self.pairs.items()):
            r=dict(zip(keys,key),trials=g['trials'],paired_successful_contacts=g['paired_successful_contacts'])
            n=g['paired_successful_contacts']
            for k in ('ev_ms','vla_deg','fair'):
                for side in ('before','after'):
                    r[side+'_'+k]=g[side+'_'+k]/n if n else None
            rows.append(r)
        write_csv(out/'bat_pivot_orientation_pairs.csv',keys+['trials','paired_successful_contacts']+[p+k for k in ('ev_ms','vla_deg','fair') for p in ('before_','after_')],rows)
        keys=keys+['before_search_status','after_search_status','before_collision_status','after_collision_status','before_direction','after_direction']
        write_csv(out/'bat_pivot_orientation_transitions.csv',keys+['trials'],[dict(zip(keys,k),trials=n) for k,n in sorted(self.transitions.items())])
        keys=['scope','variant','orientation','model','cohort','ev_band_ms','vla_band_deg']
        write_csv(out/'bat_pivot_orientation_ev_vla.csv',keys+['contacts','cohort_contacts','fraction'],[
            dict(zip(keys,k),contacts=n,cohort_contacts=self.histogram_totals[k[:5]],
                 fraction=n/self.histogram_totals[k[:5]]) for k,n in sorted(self.histogram.items())])


def run(input_dir,db_path,output,variant='observed_sigma_0.005_r0.9',gyro_sign=0):
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    coverage=audit_source(input_dir,output/'source_audit',[variant])
    db=sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro',uri=True)
    spin=None
    try:
        db.execute('PRAGMA query_only=ON');db.execute('BEGIN')
        spin=PitchSpinSource(db_path,gyro_sign,output/'invalid_pitch_spin.csv')
        checks=defaultdict(float);counts=Counter();aggregate=Aggregate();last_key=None;state=None
        models=[m for m in configs((.00015,)) if m['name'] in MODEL_NAMES]
        with (Path(input_dir)/'observation_update_inputs.jsonl').open() as pf, \
             (Path(input_dir)/'observation_update_contacts.csv').open(newline='') as cf:
            for raw,source_row in zip(pf,csv.DictReader(cf)):
                if source_row['variant']!=variant:continue
                saved=json.loads(raw)['plan'];row=audit(source_row,checks)
                key=(row['game_id'],row['count_seq'])
                if key!=last_key:
                    records=db.execute('SELECT state_version,state_source,invalid_reason,state_json FROM player_game_contact_state WHERE game_id=? AND count_seq=?',key).fetchall()
                    if len(records)!=1 or records[0][:3]!=(1,SOURCE,None):raise ValueError(f'invalid_or_missing_contact_state:{key}')
                    state=json.loads(records[0][3]);validate_state(state);last_key=key
                truth=state['input'];target=saved['forecast_position_world_m']
                side=1 if saved['ideal_axis_world'][0]>0 else -1
                if side!=truth['handedness_sign']:raise ValueError('db_handedness_mismatch')
                for i,a in [(0,'x'),(2,'z')]:
                    check(checks,'db_target_'+a+'_m',abs(target[i]-truth['ball_position_world_m'][i]-row['forecast_error_'+a+'_m']))
                check(checks,'db_clock_sec',abs(saved['nominal_time_sec']-truth['reference_time_sec_since_release']-row['forecast_time_error_ms']/1000))
                if row['status']=='contact':
                    check(checks,'db_ball_velocity_ms',norm(sub(vec(row,'ball_velocity_'),truth['ball_velocity_world_ms'])))
                spin_row = dict(game_id=key[0],count_seq=key[1],variant=variant,
                    **{'ball_velocity_'+a+'_ms':v for a,v in zip('xyz',truth['ball_velocity_world_ms'])})
                incoming_spin = spin.get(spin_row)
                plans=dict(saved_up=saved,matched_down=counterfactual(saved))
                for name,plan in plans.items():
                    motion=motion_for(state,plan)
                    check(checks,name+'_anchor_position_m',norm(sub(motion.point,saved['actual_point_world_m'])))
                    check(checks,name+'_anchor_velocity_ms',norm(sub(motion.velocity,saved['actual_velocity_world_ms'])))
                check(checks,'omega_magnitude_rad_s',abs(norm(saved['actual_omega_world_rad_s'])-norm(plans['matched_down']['actual_omega_world_rad_s'])))
                changed=evaluate(row,state,plans['matched_down'],checks)
                rows=dict(saved_up=row,matched_down=changed);results={}
                for name,r in rows.items():
                    results[name],eligibility=event(r,checks,models,incoming_spin,None,include_ground=False)
                    counts[name+':'+eligibility]+=1
                aggregate.add(row,plans,rows,results);counts['paired_trials']+=1
        if counts['paired_trials']!=coverage['selected_rows']:raise ValueError('selected_coverage_mismatch')
        aggregate.write(output)
        write_csv(output/'bat_pivot_orientation_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
        write_csv(output/'bat_pivot_orientation_coverage.csv',['metric','trials'],[dict(metric=k,trials=v) for k,v in sorted(counts.items())])
        (output/'bat_pivot_orientation_metadata.json').write_text(json.dumps(dict(version=1,variant=variant,
            source_coverage=coverage,spin=spin.metadata(),models=list(MODEL_NAMES),new_searches=counts['paired_trials'],
            note='Saved baseline contacts reused; one timed search per matched-down trial. Anchor is actual execution reference point shared by both conditions. Forecast, nominal clock, actual anchor P/V and omega magnitude fixed. Pivot translation changes to compensate Y rotation of axis/omega. No ground or zero-delay replay. Hypothesis only; not an observed human pose or calibrated correction.'),indent=2)+'\n')
        return dict(counts)
    finally:
        if spin is not None:spin.close()
        db.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,required=True);p.add_argument('--db',type=Path,required=True)
    p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--variant',default='observed_sigma_0.005_r0.9')
    p.add_argument('--gyro-sign',type=int,choices=(-1,0,1),default=0);a=p.parse_args()
    try:print(run(a.input_dir,a.db,a.output_dir,a.variant,a.gyro_sign))
    except (OSError,ValueError,KeyError,TypeError,sqlite3.Error) as exc:p.exit(1,f'Trial failed: {exc}\n')
