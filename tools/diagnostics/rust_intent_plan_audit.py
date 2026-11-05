#!/usr/bin/env python3
"""Compare Rust shadow intent plans in state_json with Python on identical forecast inputs."""
import argparse
from collections import Counter, defaultdict
from dataclasses import asdict
import json
import math
from pathlib import Path
import sqlite3

from batting_intent import BattedBallIntent, PlanningInput, frozen_planner, FROZEN_POLICIES
from contact_collision_probe import write_csv
from contact_state_probe import SOURCE
from diagnostic_db import add_db_argument
from execution_contact_posture import digest, same
from swing_execution import SwingPlan

PREFIX = 'rust_intent_plan'
SETTINGS = dict(tilt_deg=15., yaw_batter_deg=0., speed_ratio=1., duration_sec=.18, anchor_fraction=.5)


def prediction_input(prediction):
    return PlanningInput(tuple(prediction['forecast']['position_world_m']), prediction['forecast']['time_sec'],
        prediction['planned_attack_angle_deg'], prediction['bat_speed_ms'], prediction['handedness_sign'],
        prediction['execution_angle_error_deg'])


def compare_shadow(state, planner, checks):
    prediction = state['swing_prediction']
    if prediction['version'] != 1 or prediction['source'] != 'expected_pitch_prior_commit_observation_v1' or prediction['invalid_reason'] is not None:
        raise ValueError('shadow_requires_valid_prediction')
    raw = prediction['input']
    inputs = prediction_input(raw)
    shadow = state['intent_plans']
    if shadow['version'] != 1 or shadow['source'] != 'forecast_intent_shadow_v1':
        raise ValueError('unsupported_intent_shadow_contract')
    same(asdict(inputs), shadow['input'], checks, 'shadow_input')
    same(SETTINGS, shadow['settings'], checks, 'shadow_settings')
    available = raw['observation']['time_sec']
    same(available, shadow['available_time_sec'], checks, 'shadow_available_clock')
    records = shadow['plans']
    if len(records) != 2 or {r['intent'] for r in records} != {i.value for i in BattedBallIntent}:
        raise ValueError('missing_or_duplicate_shadow_intents')
    result = []
    for record in records:
        intent = BattedBallIntent(record['intent'])
        built = planner.plan(inputs, intent)
        same(built.record(), record['plan'], checks, 'rust_python_plan')
        reason = None; execution = None
        if raw['residual_execution_delay_sec'] != 0:
            reason = 'unsupported_residual_execution_delay'
        elif not math.isfinite(available) or available < 0 or available >= inputs.forecast_time_sec:
            reason = 'invalid_plan_available_clock'
        else:
            try:
                execution = SwingPlan.from_saved(built.forecast_plan)
                if execution.start_time_sec < available - 1e-10:
                    reason = 'execution_starts_before_plan'; execution = None
            except ValueError as exc:
                if str(exc) != 'execution_starts_before_release':
                    raise
                reason = str(exc)
        if record['invalid_reason'] != reason:
            raise ValueError('rust_python_clock_reason_mismatch')
        if execution is None:
            if record['execution_plan'] is not None:
                raise ValueError('invalid_clock_with_execution_plan')
        else:
            same(execution.record(), record['execution_plan'], checks, 'rust_python_execution_plan')
        result.append(dict(intent=intent.value, status=reason or 'matched', aim_z_offset_m=built.aim_z_offset_m,
                           forecast_height_m=inputs.forecast_position_world_m[2], forecast_time_sec=inputs.forecast_time_sec))
    return result


def run(db_path, first, last, output):
    if first > last:
        raise ValueError('invalid_game_range')
    output = Path(output)
    if output.exists():
        raise FileExistsError(output)
    planner = frozen_planner()
    coverage = Counter(); checks = defaultdict(float); details = []
    db = sqlite3.connect(Path(db_path).resolve().as_uri() + '?mode=ro', uri=True)
    try:
        db.execute('PRAGMA query_only=ON'); db.execute('BEGIN')
        rows = db.execute('SELECT game_id,count_seq,state_version,state_source,invalid_reason,state_json '
                          'FROM player_game_contact_state WHERE game_id BETWEEN ? AND ? ORDER BY game_id,count_seq', (first, last))
        seen = set()
        for game, seq, version, source, invalid, payload in rows:
            if (game, seq) in seen:
                raise ValueError('duplicate_shadow_pitch')
            seen.add((game, seq)); coverage['states'] += 1
            if version != 1 or source != SOURCE:
                coverage['unsupported_states'] += 1; continue
            if invalid is not None or payload is None:
                coverage['outer_invalid_states'] += 1; continue
            state = json.loads(payload)
            if state.get('intent_plans') is None:
                prediction = state.get('swing_prediction')
                status = 'prediction_invalid_states' if prediction and prediction.get('invalid_reason') is not None else 'missing_intent_plan_states'
                coverage[status] += 1; continue
            plans = compare_shadow(state, planner, checks)
            coverage['matched_states'] += 1
            for plan in plans:
                details.append(dict(game_id=game, count_seq=seq, **plan))
                coverage['matched_plans'] += 1
                coverage['matched_execution_plans' if plan['status'] == 'matched' else 'matched_clock_invalid_plans'] += 1
    finally:
        db.close()
    if not coverage['states']:
        raise ValueError('no_contact_state_rows_in_range')
    metadata = dict(version=1, status='matched_available_shadow_plans' if coverage['matched_plans'] else 'no_comparable_shadow_plans',
        db_path=str(db_path), first_game_id=first, last_game_id=last, coverage=dict(coverage), settings=SETTINGS,
        inputs={str(FROZEN_POLICIES): digest(FROZEN_POLICIES)}, new_searches=0, collision_replays=0,
        db_access='read_only_transaction', limitations=[
            'New Rust recordings use the existing saved expected-prior forecast; the Python observation-update model is not ported here.',
            'This checks identical-input planning parity, not contact performance or independent strategic effectiveness.',
            'Old rows without intent_plans are counted as missing, never populated from contact outcomes.',
            'Both intents are diagnostic alternatives; neither changes live swing/take, RNG, contact or batting outcomes.',
            'Checked numeric residuals must stay within 1e-8. Missing/invalid states remain visible in coverage.'
        ])
    output.mkdir(parents=True, exist_ok=False)
    write_csv(output / (PREFIX + '_coverage.csv'), ['metric', 'count'],
              [dict(metric=k, count=v) for k, v in sorted(coverage.items())])
    write_csv(output / (PREFIX + '_checks.csv'), ['metric', 'max_error'],
              [dict(metric=k, max_error=v) for k, v in sorted(checks.items())])
    write_csv(output / (PREFIX + '_details.csv'), ['game_id', 'count_seq', 'intent', 'status', 'aim_z_offset_m', 'forecast_height_m', 'forecast_time_sec'], details)
    (output / (PREFIX + '_metadata.json')).write_text(json.dumps(metadata, indent=2) + '\n')
    return dict(status=metadata['status'], **coverage, new_searches=0, collision_replays=0)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    add_db_argument(parser)
    parser.add_argument('--first-game-id', type=int, required=True)
    parser.add_argument('--last-game-id', type=int, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(run(args.db, args.first_game_id, args.last_game_id, args.output_dir), indent=2))
    except (OSError, ValueError, KeyError, TypeError, sqlite3.Error) as exc:
        parser.exit(1, f'Rust intent-plan audit failed: {exc}\n')
