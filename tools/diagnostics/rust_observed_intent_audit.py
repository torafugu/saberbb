#!/usr/bin/env python3
"""Audit Rust observation update and separate coach/player decisions against Python."""
import argparse
from collections import Counter, defaultdict
import copy
import json
from pathlib import Path
import sqlite3

from batting_intent import frozen_planner, FROZEN_POLICIES
from contact_collision_probe import write_csv
from contact_state_probe import SOURCE
from diagnostic_db import add_db_argument
from execution_contact_posture import digest, same
from observation_update_probe import validate_history, standard_noise, observe, estimate
from rust_intent_plan_audit import compare_shadow

PREFIX = 'rust_observed_intent'


def decide(strategy, preference):
    if preference not in ('contact_first', 'line_drive'):
        raise ValueError('unsupported_batter_preference')
    choices = dict(let_batter_decide=(preference, 'batter_preference'),
                   put_ball_in_play=('contact_first', 'manager_contact_priority'),
                   seek_line_drive=('line_drive', 'manager_line_drive_priority'))
    if strategy not in choices:
        raise ValueError('unsupported_batting_strategy')
    intent, reason = choices[strategy]
    return dict(strategy=strategy, preferences=dict(preferred_intent=preference), selected_intent=intent, reason=reason)


def compare_observed(state, game, seq, planner, checks):
    record = state['observed_intent_plans']
    if (record['version'], record['source'], record['game_id'], record['count_seq']) != (1, 'observed_intent_shadow_v1', game, seq):
        raise ValueError('observed_shadow_identity_mismatch')
    decision = record['decision']
    expected_decision = decide(decision['strategy'], decision['preferences']['preferred_intent'])
    same(expected_decision, decision, checks, 'batting_decision')
    prediction = state['swing_prediction']
    if (prediction['version'], prediction['source']) != (1, 'expected_pitch_prior_commit_observation_v1'):
        raise ValueError('unsupported_saved_prediction')
    if prediction['invalid_reason'] is not None or prediction['input'] is None:
        raise ValueError('observed_shadow_requires_valid_prediction')
    p = prediction['input']
    same(.005, record['sensor_sigma_m'], checks, 'sensor_sigma_m')
    same(p['batting_eye'], record['eye'], checks, 'sensor_eye')
    noise = standard_noise(game, seq)
    same(noise, record['standard_noise'], checks, 'standard_noise')
    error = None
    try:
        history = validate_history(p)
        observations = observe(history, .005, p['batting_eye'], noise)
        forecast, velocity, acceleration = estimate(observations)
    except ValueError as exc:
        error = str(exc)
    if record['invalid_reason'] != error:
        raise ValueError('observed_update_status_mismatch')
    if error:
        if record['estimate'] is not None or record['intent_plans'] is not None:
            raise ValueError('failed_update_has_fallback_plan')
        return [], error
    same(dict(observations=observations, velocity=velocity, acceleration=acceleration, forecast=forecast),
         record['estimate'], checks, 'observed_update')
    updated = copy.deepcopy(prediction)
    updated['input']['forecast'] = forecast
    plans = compare_shadow(dict(swing_prediction=updated, intent_plans=record['intent_plans']), planner, checks)
    for plan in plans:
        plan.update(selected=plan['intent'] == decision['selected_intent'], strategy=decision['strategy'],
                    preferred_intent=decision['preferences']['preferred_intent'], decision_reason=decision['reason'])
    return plans, None


def run(db_path, first, last, output):
    if first > last:
        raise ValueError('invalid_game_range')
    output = Path(output)
    if output.exists():
        raise FileExistsError(output)
    planner = frozen_planner(); coverage = Counter(); checks = defaultdict(float); details = []
    db = sqlite3.connect(Path(db_path).resolve().as_uri() + '?mode=ro', uri=True)
    try:
        db.execute('PRAGMA query_only=ON'); db.execute('BEGIN')
        rows = db.execute('SELECT game_id,count_seq,state_version,state_source,invalid_reason,state_json '
                          'FROM player_game_contact_state WHERE game_id BETWEEN ? AND ? ORDER BY game_id,count_seq', (first, last))
        seen = set()
        for game, seq, version, source, invalid, payload in rows:
            if (game, seq) in seen:
                raise ValueError('duplicate_observed_pitch')
            seen.add((game, seq)); coverage['states'] += 1
            if version != 1 or source != SOURCE:
                coverage['unsupported_states'] += 1; continue
            if invalid is not None or payload is None:
                coverage['outer_invalid_states'] += 1; continue
            state = json.loads(payload)
            if state.get('observed_intent_plans') is None:
                prediction = state.get('swing_prediction')
                status = 'prediction_invalid_states' if prediction and prediction.get('invalid_reason') is not None else 'missing_observed_intent_states'
                coverage[status] += 1; continue
            plans, error = compare_observed(state, game, seq, planner, checks)
            coverage['matched_update_states'] += 1
            if error:
                coverage['update_invalid:' + error] += 1; continue
            coverage['matched_forecasts'] += 1
            for plan in plans:
                details.append(dict(game_id=game, count_seq=seq, **plan))
                coverage['matched_plans'] += 1
                coverage['matched_execution_plans' if plan['status'] == 'matched' else 'matched_clock_invalid_plans'] += 1
                if plan['selected']:
                    coverage['selected:' + plan['intent']] += 1
    finally:
        db.close()
    if not coverage['states']:
        raise ValueError('no_contact_state_rows_in_range')
    metadata = dict(version=1, status='matched_available_observed_plans' if coverage['matched_plans'] else 'no_comparable_observed_plans',
        db_path=str(db_path), first_game_id=first, last_game_id=last, coverage=dict(coverage),
        sensor_sigma_m=.005, noise_contract='observation_sensor_v1; SHA256 key, CPython integer-seeded MT19937 random/gauss',
        inputs={str(FROZEN_POLICIES): digest(FROZEN_POLICIES)}, new_searches=0, collision_replays=0,
        db_access='read_only_transaction', limitations=[
            'Coach objectives and per-player preferences are explicitly configured, not fitted or automatically inferred from game outcomes.',
            'Both alternatives are diagnostic; selected intent is recorded without changing live batting or game RNG.',
            'Observation-update and planning parity are checked on identical inputs; no physical contact performance is evaluated here.',
            'Existing execution-angle error is retained from the prior branch, matching the Python observation-update experiment.',
            'Old rows are missing, never reconstructed from actual endpoints; invalid updates do not fall back to the prior.',
            'Sensor sigma and eye scaling are uncalibrated hypotheses. World-height cutoffs, player/game dependence and physics limits remain.'
        ])
    output.mkdir(parents=True, exist_ok=False)
    write_csv(output / (PREFIX + '_coverage.csv'), ['metric', 'count'], [dict(metric=k, count=v) for k,v in sorted(coverage.items())])
    write_csv(output / (PREFIX + '_checks.csv'), ['metric', 'max_error'], [dict(metric=k, max_error=v) for k,v in sorted(checks.items())])
    fields = ['game_id', 'count_seq', 'intent', 'status', 'aim_z_offset_m', 'forecast_height_m', 'forecast_time_sec',
              'selected', 'strategy', 'preferred_intent', 'decision_reason']
    write_csv(output / (PREFIX + '_details.csv'), fields, details)
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
        parser.exit(1, f'Rust observed-intent audit failed: {exc}\n')
