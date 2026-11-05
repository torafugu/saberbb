#!/usr/bin/env python3
"""Verify explicit seed/input/DB provenance before frozen Rust response audit (SABERBB_DB)."""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
import struct

from diagnostic_db import db_from_env
from execution_contact_posture import digest
from batting_intent import FROZEN_POLICIES
import rust_collision_response_audit as response


def derive_seed(master,game):
    raw=b'saberbb:holdout:v1\0'+struct.pack('<QI',master,game)
    return int.from_bytes(hashlib.sha256(raw).digest()[:8],'little')


def integer(value,name,maximum):
    if not isinstance(value,str) or not value.isascii() or not value.isdecimal():raise ValueError('invalid_'+name)
    number=int(value)
    if not 0<=number<=maximum:raise ValueError('invalid_'+name)
    return number


def load_run(folder):
    folder=Path(folder);path=folder/'manifest.json';manifest=json.loads(path.read_text())
    if (manifest['version'],manifest['source'],manifest['status'])!=(1,'seeded_intent_holdout_v1','complete'):
        raise ValueError('run_not_complete_or_unsupported')
    if manifest['completed_rounds']!=manifest['requested_rounds'] or manifest['completed_rounds']<1 or manifest.get('error') is not None:
        raise ValueError('run_rounds_incomplete')
    master=integer(manifest['master_seed'],'master_seed',2**64-1)
    if manifest['model']!=response.collision.MODEL:raise ValueError('holdout_model_mismatch')
    if manifest['frozen_policy_sha256']!=digest(FROZEN_POLICIES):raise ValueError('holdout_frozen_policy_mismatch')
    if not 0<manifest['max_pitches']<65535:raise ValueError('holdout_budget_invalid')
    expected={};inputs={str(path):digest(path)};rounds=set()
    for game in manifest['games']:
        game_id=game['game_id']
        if type(game_id) is not int or not 0<game_id<2**32 or game_id<=manifest['exclude_through_game_id'] or game_id in expected:
            raise ValueError('duplicate_or_excluded_holdout_game')
        if game['status']!='saved' or game['contacts_sha256'] is None or game['result_sha256'] is None:
            raise ValueError('unsaved_holdout_game')
        seed=integer(game['game_seed'],'game_seed',2**64-1)
        if seed!=derive_seed(master,game_id):raise ValueError('derived_seed_mismatch')
        name=f'cases/game_{game_id}.json'
        if game['input_file']!=name:raise ValueError('invalid_case_path')
        case_path=folder/name;hash_value=digest(case_path)
        if hash_value!=game['input_sha256']:raise ValueError('case_hash_mismatch')
        inputs[str(case_path)]=hash_value;case=json.loads(case_path.read_text())
        if (case['version'],case['schedule']['id'],case['game_seed'])!=(1,game_id,game['game_seed']):raise ValueError('case_identity_mismatch')
        if case['engine_source_sha256']!=manifest['engine_source_sha256']:raise ValueError('mixed_engine_fingerprints')
        if case['build_sha256']!=manifest['build_sha256']:raise ValueError('mixed_build_fingerprints')
        if case['max_pitches']!=manifest['max_pitches']:raise ValueError('mixed_pitch_budgets')
        if case['batting_settings']!=dict(away_strategy='let_batter_decide',home_strategy='let_batter_decide',player_preferences={}):
            raise ValueError('holdout_strategy_changed')
        rounds.add((case['schedule']['season'],case['schedule']['round_seq']))
        if type(game['contact_states']) is not int or game['contact_states']<0:raise ValueError('invalid_contact_state_count')
        expected[game_id]=dict(sha256=game['contacts_sha256'],count=game['contact_states'])
    if not expected or len(rounds)!=manifest['requested_rounds']:raise ValueError('holdout_round_population_mismatch')
    if manifest.get('round_plan') is not None:
        plan=manifest['round_plan'];planned={}
        if len(plan)!=manifest['requested_rounds']:raise ValueError('declared_round_plan_size_mismatch')
        for round_info in plan:
            for game in round_info['game_ids']:
                if game in planned:raise ValueError('duplicate_declared_plan_game')
                planned[game]=(round_info['season'],round_info['round_seq'])
        if set(planned)!=set(expected):raise ValueError('declared_game_population_mismatch')
        for game in manifest['games']:
            case=json.loads((folder/game['input_file']).read_text())
            if planned[game['game_id']]!=(case['schedule']['season'],case['schedule']['round_seq']):raise ValueError('declared_round_identity_mismatch')
    return manifest,expected,inputs


def run(db_path,run_dir,output):
    output=Path(output)
    if output.exists():raise FileExistsError(output)
    manifest,expected,inputs=load_run(run_dir)
    # The contact hashes are checked inside the very same read snapshot as response parity.
    result=response.run(db_path,min(expected),max(expected),output,game_ids=set(expected),expected_contacts=expected)
    meta_path=output/(response.PREFIX+'_metadata.json');metadata=json.loads(meta_path.read_text())
    metadata.update(holdout_manifest=manifest,holdout_inputs_sha256=inputs,
        holdout_assessment='explicit_seed_and_frozen_input_provenance_verified; not proof that an unrecorded historical seed was never used',
        seed_provenance_limitations=[
            'Declared exclusion boundary and seed are fixed before this run; prior unseeded history has no recoverable seed ledger.',
            'This verifies generated DB diagnostic rows, not independent rosters/players or statistical independence across games.',
            'Full replay hashes can be checked separately with intent_holdout replay using the same engine/build fingerprint.',
            'Failed or incomplete runs are rejected; do not remove failed games or reuse a holdout for tuning.'
        ])
    metadata['limitations']=[s for s in metadata['limitations'] if not s.startswith('An independent-seed holdout remains pending')]
    meta_path.write_text(json.dumps(metadata,indent=2,allow_nan=False)+'\n')
    return dict(result,provenance_games_verified=len(expected),master_seed=manifest['master_seed'])


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--run-dir',type=Path,required=True);p.add_argument('--output-dir',type=Path,required=True);a=p.parse_args()
    try:print(json.dumps(run(db_from_env(),a.run_dir,a.output_dir),indent=2))
    except (OSError,ValueError,KeyError,TypeError,sqlite3.Error) as exc:p.exit(1,f'Seeded holdout audit failed: {exc}\n')
