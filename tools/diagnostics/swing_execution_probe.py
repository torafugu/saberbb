#!/usr/bin/env python3
"""Saved constant-speed vs per-pitch execution pulse; diagnostic-only paired trials."""
import argparse
import csv
import json
import math
import sqlite3
import tempfile
from collections import Counter, defaultdict
from itertools import product
from pathlib import Path

from bat_axis_origin_audit import run as audit_source, check
from bat_pivot_orientation_probe import Aggregate, evaluate, MODEL_NAMES
from contact_collision_probe import write_csv
from contact_state_probe import SOURCE, validate_state
from diagnostic_db import add_db_argument
from finite_barrel_probe import configs, event
from forecast_contact_geometry import audit, vec
from pitch_spin_source import PitchSpinSource
from rotating_bat_probe import norm, sub
from swing_execution import SwingExecution, ExecutionMotion


def scenario_name(effort,fraction,offset):
    return f'effort_{effort:g}_anchor_{fraction:g}_delay_{offset*1000:g}ms'


def write_aggregates(output,groups):
    # Reuse the established cohort/transition logic and preserve scenario as a separate column.
    with tempfile.TemporaryDirectory(prefix='swing_execution_groups_') as tmp:
        root=Path(tmp)
        for scenario,aggregate in groups.items():
            folder=root/scenario;folder.mkdir();aggregate.write(folder)
        for suffix in ('summary','pairs','transitions','ev_vla'):
            with (output/f'swing_execution_{suffix}.csv').open('x',newline='') as f:
                writer=None
                for scenario in groups:
                    invalid=Counter();invalid_geometry=Counter()
                    if suffix=='summary':
                        with (root/scenario/'bat_pivot_orientation_transitions.csv').open(newline='') as transitions:
                            for r in csv.DictReader(transitions):
                                if r['after_search_status'].startswith('execution_starts_'):
                                    key=tuple(r[k] for k in ('scope','variant','stratum','band','model'))
                                    invalid[key]+=int(r['trials'])
                                    invalid_geometry[key]+=int(r['trials'])*(r['before_search_status']=='contact')
                    with (root/scenario/f'bat_pivot_orientation_{suffix}.csv').open(newline='') as source:
                        reader=csv.DictReader(source)
                        extra=['execution_clock_invalid_trials','evaluable_trials','evaluable_geometry_contacts','contact_fraction_of_evaluable_trials'] if suffix=='summary' else []
                        if writer is None:
                            writer=csv.DictWriter(f,fieldnames=['scenario']+reader.fieldnames+extra);writer.writeheader()
                        for r in reader:
                            if suffix=='summary':
                                key=tuple(r[k] for k in ('scope','variant','stratum','band','model'));n=invalid[key]
                                evaluated=int(r['trials'])-n
                                contacts=int(r['geometry_contacts'])-(invalid_geometry[key] if r['orientation']=='saved_constant' else 0)
                                r.update(execution_clock_invalid_trials=n,evaluable_trials=evaluated,evaluable_geometry_contacts=contacts,
                                    contact_fraction_of_evaluable_trials=contacts/evaluated if evaluated else None)
                            writer.writerow(dict(scenario=scenario,**r))


def run(input_dir,db_path,output,variant='observed_sigma_0.005_r0.9',gyro_sign=0,
        efforts=(1.,.85),duration=.18,fractions=(.5,),offsets=(0.,)):
    if (not efforts or len(set(efforts))!=len(efforts) or not fractions or len(set(fractions))!=len(fractions)
        or not offsets or len(set(offsets))!=len(offsets) or gyro_sign not in (-1,0,1)):
        raise ValueError('invalid_execution_scenarios')
    scenarios={scenario_name(e,f,o):(e,f,o) for e,f,o in product(efforts,fractions,offsets)}
    if len(scenarios)!=len(efforts)*len(fractions)*len(offsets):raise ValueError('scenario_label_collision')
    for e,f,o in scenarios.values():SwingExecution(0,0,10.,e,duration,f,o)
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    coverage=audit_source(input_dir,output/'source_audit',[variant])
    db=sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro',uri=True);spin=None
    try:
        db.execute('PRAGMA query_only=ON');db.execute('BEGIN')
        spin=PitchSpinSource(db_path,gyro_sign,output/'invalid_pitch_spin.csv')
        checks=defaultdict(float);counts=Counter(new_searches=0)
        groups={name:Aggregate('saved_constant','execution') for name in scenarios}
        models=[m for m in configs((.00015,)) if m['name'] in MODEL_NAMES]
        with (Path(input_dir)/'observation_update_inputs.jsonl').open() as pf, \
             (Path(input_dir)/'observation_update_contacts.csv').open(newline='') as cf, \
             (output/'swing_execution_states.jsonl').open('x') as sf:
            for raw,source_row in zip(pf,csv.DictReader(cf)):
                if source_row['variant']!=variant:continue
                source_record=json.loads(raw);plan=source_record['plan'];row=audit(source_row,checks)
                key=(row['game_id'],row['count_seq'])
                records=db.execute('SELECT state_version,state_source,invalid_reason,state_json FROM player_game_contact_state WHERE game_id=? AND count_seq=?',key).fetchall()
                if len(records)!=1 or records[0][:3]!=(1,SOURCE,None):raise ValueError(f'invalid_or_missing_contact_state:{key}')
                state=json.loads(records[0][3]);validate_state(state);truth=state['input']
                observations=source_record.get('observations')
                available_time=(max(o['time_sec'] for o in observations) if observations else
                    state['swing_prediction']['input']['observation']['time_sec'])
                if not math.isfinite(available_time) or not 0<=available_time<plan['nominal_time_sec']:
                    raise ValueError('invalid_plan_available_clock')
                side=1 if plan['ideal_axis_world'][0]>0 else -1
                if side!=truth['handedness_sign']:raise ValueError('db_handedness_mismatch')
                for i,a in [(0,'x'),(2,'z')]:
                    check(checks,'db_target_'+a+'_m',abs(plan['forecast_position_world_m'][i]-truth['ball_position_world_m'][i]-row['forecast_error_'+a+'_m']))
                check(checks,'db_clock_sec',abs(plan['nominal_time_sec']-truth['reference_time_sec_since_release']-row['forecast_time_error_ms']/1000))
                if row['status']=='contact':check(checks,'db_ball_velocity_ms',norm(sub(vec(row,'ball_velocity_'),truth['ball_velocity_world_ms'])))
                incoming_spin=spin.get(dict(game_id=key[0],count_seq=key[1],variant=variant,
                    **{'ball_velocity_'+a+'_ms':v for a,v in zip('xyz',truth['ball_velocity_world_ms'])}))
                baseline,reason=event(row,checks,models,incoming_spin,None,include_ground=False)
                counts['saved_constant:'+reason]+=1
                for name,(e,f,o) in scenarios.items():
                    invalid=None
                    try:execution=SwingExecution(*key,plan['nominal_time_sec'],e,duration,f,o)
                    except ValueError as exc:
                        if str(exc)!='execution_starts_before_release':raise
                        invalid='execution_starts_before_release'
                    if invalid is None and execution.start_time_sec<available_time-1e-10:
                        invalid='execution_starts_before_plan'
                    if invalid is not None:
                        reason='not_evaluated:'+invalid
                        changed=dict(row,status=invalid,collision_status=reason)
                        results={m:dict(collision_status=reason,ev_ms=None) for m in MODEL_NAMES}
                        counts[name+':'+reason]+=1
                        sf.write(json.dumps(dict(scenario=name,game_id=key[0],count_seq=key[1],invalid_reason=invalid,
                            plan_available_time_sec=available_time,nominal_time_sec=plan['nominal_time_sec'],
                            effort=e,duration_sec=duration,anchor_fraction=f,start_offset_sec=o))+'\n')
                        groups[name].add(row,dict(saved_constant=plan,execution=plan),
                            dict(saved_constant=row,execution=changed),dict(saved_constant=baseline,execution=results))
                        continue
                    motion=ExecutionMotion(state,plan,execution)
                    # With zero start offset, effort changes speed but keeps the nominal pose anchored.
                    point,axis=motion.pose(o,0.)
                    check(checks,'execution_reference_position_m',norm(sub(point,plan['actual_point_world_m'])))
                    check(checks,'execution_reference_axis',norm(sub(axis,plan['actual_axis_world'])))
                    changed=evaluate(row,state,plan,checks,motion=motion)
                    counts['new_searches']+=1
                    results,reason=event(changed,checks,models,incoming_spin,None,include_ground=False)
                    counts[name+':'+reason]+=1
                    sf.write(json.dumps(dict(scenario=name,execution=execution.record(),plan_available_time_sec=available_time,
                        lever_m=plan['lever_m'],half_length_m=plan['half_length_m'],pivot_world_m=plan['pivot_world_m'],
                        reference_point_world_m=plan['actual_point_world_m'],reference_axis_world=plan['actual_axis_world'],
                        base_omega_world_rad_s=plan['actual_omega_world_rad_s'],
                        base_pivot_velocity_world_ms=plan['pivot_velocity_world_ms'],
                        entry_time_sec=changed.get('entry_since_release_sec'),
                        entry_gain=execution.gain(motion.phase(changed['entry_time_offset_ms']/1000,
                            plan['nominal_time_sec']-truth['reference_time_sec_since_release'])) if changed['status']=='contact' else None,
                        instantaneous_entry_omega_world_rad_s=[changed.get('omega_'+a+'_rad_s') for a in 'xyz'] if changed['status']=='contact' else None),allow_nan=False)+'\n')
                    groups[name].add(row,dict(saved_constant=plan,execution=plan),
                        dict(saved_constant=row,execution=changed),dict(saved_constant=baseline,execution=results))
                counts['source_selected_trials']+=1
        if counts['source_selected_trials']!=coverage['selected_rows']:raise ValueError('selected_coverage_mismatch')
        write_aggregates(output,groups)
        write_csv(output/'swing_execution_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
        write_csv(output/'swing_execution_coverage.csv',['metric','trials'],[dict(metric=k,trials=v) for k,v in sorted(counts.items())])
        (output/'swing_execution_metadata.json').write_text(json.dumps(dict(version=1,variant=variant,
            source_coverage=coverage,spin=spin.metadata(),duration_sec=duration,
            scenarios={k:dict(effort=e,anchor_fraction=f,start_offset_sec=o) for k,(e,f,o) in scenarios.items()},
            new_searches=counts['new_searches'],
            profile='sin(pi*u)^2 on 0<u<1, zero outside; same analytic integral drives rotation and pivot translation; peak omega and pivot velocity scaled by effort',
            note='Explicit sensitivity scenarios, not sampled per-pitch effort frequencies. Reference time from saved forecast plan, no actual endpoint used to choose execution state. Baseline reused once per pitch; one timed search per scenario; instantaneous omega passed to collision solver. Frozen instantaneous collision law unchanged. Output scopes/scenarios/models overlap. No ground or zero-delay replay; no game/RNG/DB writes.'),indent=2)+'\n')
        return dict(counts)
    finally:
        if spin is not None:spin.close()
        db.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,required=True);add_db_argument(p)
    p.add_argument('--output-dir',type=Path,required=True);p.add_argument('--variant',default='observed_sigma_0.005_r0.9')
    p.add_argument('--gyro-sign',type=int,choices=(-1,0,1),default=0)
    p.add_argument('--efforts',type=float,nargs='+',default=[1.,.85]);p.add_argument('--duration-sec',type=float,default=.18)
    p.add_argument('--anchor-fractions',type=float,nargs='+',default=[.5]);p.add_argument('--start-offset-ms',type=float,nargs='+',default=[0.])
    a=p.parse_args()
    try:print(run(a.input_dir,a.db,a.output_dir,a.variant,a.gyro_sign,a.efforts,a.duration_sec,a.anchor_fractions,[x/1000 for x in a.start_offset_ms]))
    except (OSError,ValueError,KeyError,TypeError,sqlite3.Error) as exc:p.exit(1,f'Execution trial failed: {exc}\n')
