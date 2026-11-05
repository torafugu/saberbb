#!/usr/bin/env python3
"""Audit frozen Rust finite-bat responses against Python; DB path comes from SABERBB_DB."""
import argparse
from collections import Counter,defaultdict
from dataclasses import asdict
import json
import math
from pathlib import Path
import sqlite3

from batting_intent import frozen_planner,FROZEN_POLICIES
from contact_collision_probe import write_csv
from contact_state_probe import SOURCE
from diagnostic_db import db_from_env
from execution_contact_posture import digest,same
from finite_barrel_collision import solve
from pitch_spin_source import reconstruct,InvalidPitchSpin
from rust_execution_contact_audit import compare_execution
import rust_intent_collision_probe as collision

PREFIX='rust_collision_response'


def finite_input(prepared,incoming,radius=.07):
    return dict(geometry=asdict(prepared['geometry']),vin=prepared['vin'],centerline_velocity=prepared['vbat'],
        ball_omega=incoming,bat_omega=prepared['omega'],parameters=dict(mass=.885,transverse_inertia=.885*.23**2,
            axial_inertia=.00015,cm_offset=.564-.71,barrel_radius=radius-.036,
            restitution=prepared['restitution'],mu=.2,et=.4,ball_mass=.145,ball_radius=.036,inertia_factor=.4))


def solve_input(record):
    from contact_3d import ContactGeometry
    return solve(ContactGeometry(**record['geometry']),record['vin'],record['centerline_velocity'],
        record['ball_omega'],record['bat_omega'],**record['parameters'])


def spin_record(raw,velocity):
    names=('spin_rate','spin_angle','spin_efficiency')
    if any(x is not None and not isinstance(x,(int,float)) for x in raw):
        raise ValueError('nonnumeric_saved_pitch_spin')
    # Rust serializes non-finite scalar inputs as explicit nulls, never a zero-spin fallback.
    fields={k:float(x) if isinstance(x,(int,float)) and math.isfinite(x) else None for k,x in zip(names,raw)}
    try:
        value=reconstruct(*(fields[k] for k in names),velocity,0);reason=None
    except InvalidPitchSpin as exc:
        value=None;reason=exc.reason
    return dict(input=dict(fields,gyro_sign=0),invalid_reason=reason,world_rad_s=value)


def reference_evaluation(state,row,spin,checks):
    prepared,reason=collision.prepare_contact(state,row)
    if prepared is None or spin['world_rad_s'] is None:
        return dict(input=None,response=dict(collision_status=reason or 'not_evaluated:invalid_pitch_spin'),control=None,control_checks={})
    record=finite_input(prepared,spin['world_rad_s'],state['execution_contact']['settings']['effective_radius_m'])
    local=defaultdict(float)
    result=collision.response(prepared,spin['world_rad_s'],local)
    control_input=dict(record,parameters=dict(record['parameters'],mu=0.,et=0.))
    control=solve_input(control_input)
    for k,v in local.items():checks[k]=max(checks[k],v)
    return dict(input=record,response=result,control=control,
        control_checks={k:local['control_'+k] for k in ('outgoing_ms','unchanged_spin_rad_s')})


def reference_diagnostics(state,rows,raw_spin,checks):
    spin=spin_record(raw_spin,state['input']['ball_velocity_world_ms'])
    return dict(version=1,source='intent_collision_shadow_v1',model=collision.MODEL,
        decision=state['observed_intent_plans']['decision'],spin=spin,
        probes=[dict(intent=r['intent'],selected=r['selected'],invalid_reason=None,
                     evaluation=reference_evaluation(state,r,spin,checks)) for r in rows])


def compare_collision(state,game,seq,raw_spin,planner,checks):
    rows,error=compare_execution(state,game,seq,planner,checks)
    if error:
        if state.get('intent_collision') is not None:raise ValueError('collision_without_valid_contact_diagnostics')
        return [],error
    saved=state.get('intent_collision')
    if saved is None:return [],'missing_intent_collision_states'
    expected=reference_diagnostics(state,rows,raw_spin,checks)
    if len(saved['probes'])!=2 or {p['intent'] for p in saved['probes']}!={'contact_first','line_drive'}:
        raise ValueError('missing_or_duplicate_collision_intents')
    # Ignore incidental array ordering while retaining each intent and selected identity.
    saved=dict(saved,probes=sorted(saved['probes'],key=lambda p:p['intent']))
    expected=dict(expected,probes=sorted(expected['probes'],key=lambda p:p['intent']))
    for probe in saved['probes']:
        if type(probe['selected']) is not bool:raise ValueError('invalid_selected_collision_flag')
    same(expected,saved,checks,'rust_python_collision')
    by_intent={p['intent']:p for p in saved['probes']}
    evaluated=[]
    for row in rows:
        evaluation=by_intent[row['intent']]['evaluation'];result=evaluation['response']
        inp=evaluation['input']
        evaluated.append(dict(row,game_id=game,count_seq=seq,collision_status=result['collision_status'],
            forecast_height_band_m=collision.band(row['forecast_height_m'],(.6,.9,1.2)),
            source_z_m=.71+inp['geometry']['closest_axial_m'] if inp else None,
            applied_e=inp['parameters']['restitution'] if inp else None,
            **collision.flags(row,result),**{k:result.get(k) for k in ('ev_ms','hla_deg','vla_deg','spin_rpm')}))
    return evaluated,None


def run(db_path,first,last,output):
    if first>last:raise ValueError('invalid_game_range')
    output=Path(output)
    if output.exists():raise FileExistsError(output)
    planner=frozen_planner();coverage=Counter({k:0 for k in ('states','matched_states','matched_plans','matched_responses','selected_plans','replayed_searches','python_solver_calls')})
    checks=defaultdict(float);aggregate=collision.Aggregate();details=[];seen=set()
    db=sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro',uri=True)
    try:
        db.execute('PRAGMA query_only=ON');db.execute('BEGIN')
        for game,seq,version,source,invalid,payload in db.execute('SELECT game_id,count_seq,state_version,state_source,invalid_reason,state_json '
            'FROM player_game_contact_state WHERE game_id BETWEEN ? AND ? ORDER BY game_id,count_seq',(first,last)):
            if (game,seq) in seen:raise ValueError('duplicate_collision_response_pitch')
            seen.add((game,seq));coverage['states']+=1
            if version!=1 or source!=SOURCE:coverage['unsupported_states']+=1;continue
            if invalid is not None or payload is None:coverage['outer_invalid_states']+=1;continue
            state=json.loads(payload)
            if state.get('intent_collision') is None:coverage['missing_intent_collision_states']+=1;continue
            raw=db.execute('SELECT spin_rate,spin_angle,spin_efficiency FROM player_game_pitching WHERE game_id=? AND count_seq=?',(game,seq)).fetchall()
            if len(raw)!=1:raise ValueError('pitch_spin_key_missing_or_duplicate')
            rows,error=compare_collision(state,game,seq,raw[0],planner,checks)
            if error:coverage[error]+=1;continue
            coverage['matched_states']+=1
            coverage['python_solver_calls']+=3*sum(p['evaluation']['input'] is not None for p in state['intent_collision']['probes'])
            if state['intent_collision']['spin']['invalid_reason']:coverage['invalid_spin_states']+=1
            for row in rows:
                coverage['matched_plans']+=1;coverage['selected_plans']+=row['selected']
                coverage['status:'+row['collision_status']]+=1
                coverage['replayed_searches']+=row['status'] in collision.SEARCHED
                coverage['matched_responses']+=row['collision_status']=='impulse_contact'
                details.append(row)
            aggregate.add(rows)
    finally:db.close()
    if not coverage['states']:raise ValueError('no_contact_state_rows_in_range')
    metadata=dict(version=1,status='matched_available_rust_collision_responses' if coverage['matched_plans'] else 'no_comparable_rust_collision_responses',
        first_game_id=first,last_game_id=last,db_path=str(db_path),coverage=dict(coverage),model=collision.MODEL,
        inputs={str(p):digest(p) for p in [FROZEN_POLICIES,Path(__file__),Path(collision.__file__),Path(__file__).with_name('finite_barrel_collision.py')]},
        db_access='single_read_only_transaction_for_states_and_spin',new_searches=coverage['replayed_searches'],
        collision_replays=coverage['python_solver_calls'],
        limitations=['Rust and Python use identical frozen inputs and coefficients; numerical parity is not independent physical calibration.',
            'Recorded spin inputs must match saved pitching scalars; unknown gyro is omitted (gyro_sign=0).',
            'Old states without intent_collision are missing, never reconstructed. Side/range/clock/spin/solver exclusions stay explicit.',
            'Coach/player decision, selected flags, both comparison plans, controls and physics checks are audited without changing live batting or RNG.',
            'An independent-seed holdout remains pending; keep this parity sample separate from subsequent evaluation.'])
    output.mkdir(parents=True,exist_ok=False)
    for suffix,rows,fields in [
        ('summary',aggregate.summaries(),['scope','intent','plans']),('pairs',aggregate.paired_rows(),['before_intent','after_intent']),
        ('details',details,['game_id','count_seq','intent']),
        ('coverage',[dict(metric=k,count=v) for k,v in sorted(coverage.items())],['metric','count']),
        ('checks',[dict(metric=k,max_error=v) for k,v in sorted(checks.items())],['metric','max_error'])]:
        write_csv(output/(PREFIX+'_'+suffix+'.csv'),list(rows[0]) if rows else fields,rows)
    (output/(PREFIX+'_metadata.json')).write_text(json.dumps(metadata,indent=2,allow_nan=False)+'\n')
    return dict(status=metadata['status'],**coverage)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--first-game-id',type=int,required=True);p.add_argument('--last-game-id',type=int,required=True)
    p.add_argument('--output-dir',type=Path,required=True);a=p.parse_args()
    try:print(json.dumps(run(db_from_env(),a.first_game_id,a.last_game_id,a.output_dir),indent=2))
    except (OSError,ValueError,KeyError,TypeError,sqlite3.Error) as exc:p.exit(1,f'Rust collision response audit failed: {exc}\n')
