#!/usr/bin/env python3
"""Verify Rust forward execution/contact geometry; read DB path from SABERBB_DB."""
import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
import sqlite3

from batting_intent import frozen_planner, FROZEN_POLICIES
from contact_collision_probe import write_csv
from contact_state_probe import SOURCE
from diagnostic_db import db_from_env
from execution_contact_posture import digest, same
from intent_execution_contract import evaluate, pose_samples, SETTINGS
from rust_observed_intent_audit import compare_observed
from swing_execution import SwingPlan

PREFIX = 'rust_execution_contact'


def compare_execution(state, game, seq, planner, checks):
    plans, update_error = compare_observed(state, game, seq, planner, checks)
    record = state.get('execution_contact')
    if update_error:
        if record is not None:
            raise ValueError('failed_update_has_contact_probe')
        return [], update_error
    if record is None:
        return [], 'missing_execution_contact_states'
    if (record['version'], record['source']) != (1, 'intent_execution_contact_shadow_v1'):
        raise ValueError('unsupported_execution_contact_contract')
    same(state['observed_intent_plans']['decision'], record['decision'], checks, 'contact_decision')
    same(SETTINGS, record['settings'], checks, 'contact_settings')
    probes = record['probes']
    if len(probes) != 2 or {p['intent'] for p in probes} != {'contact_first', 'line_drive'}:
        raise ValueError('missing_or_duplicate_contact_intents')
    probes = {p['intent']: p for p in probes}
    saved_plans = {p['intent']: p for p in state['observed_intent_plans']['intent_plans']['plans']}
    rows = []
    for plan in plans:
        probe = probes[plan['intent']]
        if type(probe['selected']) is not bool or probe['selected'] != plan['selected']:
            raise ValueError('selected_contact_intent_mismatch')
        row = dict(plan, search_iterations=0, time_offset_sec=None, contact_region=None)
        if plan['status'] != 'matched':
            same(dict(invalid_reason=plan['status'], samples=[], search=None), probe, checks, 'clock_invalid_probe')
        else:
            execution = SwingPlan.from_saved(saved_plans[plan['intent']]['plan']['forecast_plan'])
            try:
                samples, result = evaluate(state['input'], execution)
            except ValueError as exc:
                if str(exc) != 'invalid_contact_search_input':
                    raise
                same(dict(invalid_reason=str(exc), samples=pose_samples(state['input'], execution), search=None), probe, checks, 'invalid_search')
                row.update(status=str(exc))
            else:
                same(dict(invalid_reason=None, samples=samples, search=result), probe, checks, 'execution_contact')
                row.update(status=result['status'], search_iterations=result['iterations'],
                    time_offset_sec=result['time_offset_sec'],
                    contact_region=result['geometry']['contact_region'] if result['geometry'] else None)
        rows.append(row)
    return rows, None


def summaries(details):
    groups = defaultdict(Counter)
    for row in details:
        for scope in ['all_plans'] + (['selected_plans'] if row['selected'] else []):
            key = (scope, row['intent'], row['strategy'], row['preferred_intent'])
            groups[key][row['status']] += 1
    rows = []
    for (scope, intent, strategy, preference), counts in sorted(groups.items()):
        contact, miss = counts['contact'], counts['no_contact']
        searched = sum(counts[s] for s in ('contact', 'no_contact', 'unresolved', 'overlap_at_start'))
        rows.append(dict(scope=scope, intent=intent, strategy=strategy, preferred_intent=preference,
            plans=sum(counts.values()), searched=searched, contact=contact, no_contact=miss,
            unresolved=counts['unresolved'], overlap_at_start=counts['overlap_at_start'],
            clock_invalid=sum(v for k,v in counts.items() if k.startswith('execution_starts_') or k == 'invalid_plan_available_clock'),
            search_invalid=counts['invalid_contact_search_input'],
            contact_pct_of_searched=100*contact/searched if searched else None,
            contact_pct_of_resolved=100*contact/(contact+miss) if contact+miss else None))
    return rows


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
                raise ValueError('duplicate_execution_contact_pitch')
            seen.add((game, seq)); coverage['states'] += 1
            if version != 1 or source != SOURCE:
                coverage['unsupported_states'] += 1; continue
            if invalid is not None or payload is None:
                coverage['outer_invalid_states'] += 1; continue
            state = json.loads(payload)
            if state.get('observed_intent_plans') is None:
                coverage['missing_observed_intent_states'] += 1; continue
            plans, error = compare_execution(state, game, seq, planner, checks)
            if error:
                coverage[error] += 1; continue
            coverage['matched_states'] += 1
            for plan in plans:
                details.append(dict(game_id=game, count_seq=seq, **plan))
                coverage['matched_plans'] += 1
                coverage['status:' + plan['status']] += 1
                if plan['status'] in ('contact', 'no_contact', 'unresolved', 'overlap_at_start'):
                    coverage['replayed_searches'] += 1
                if plan['selected']:
                    coverage['selected:' + plan['intent']] += 1
    finally:
        db.close()
    if not coverage['states']:
        raise ValueError('no_contact_state_rows_in_range')
    metadata = dict(version=1, status='matched_available_execution_contacts' if coverage['matched_plans'] else 'no_comparable_execution_contacts',
        db_path=str(db_path), first_game_id=first, last_game_id=last, coverage=dict(coverage), settings=SETTINGS,
        inputs={str(FROZEN_POLICIES): digest(FROZEN_POLICIES)}, new_searches=coverage['replayed_searches'], collision_replays=0,
        db_access='read_only_transaction', limitations=[
            'Actual ball state is used only to verify geometry after forecast-only intent selection and plan construction.',
            'Two alternative intents share one pitch; all_plans and selected_plans are overlapping reports, not independent trials.',
            'Geometry uses a finite segment with effective radius 0.07m and linear ball motion within +/-0.06s of the plate reference.',
            'Contact is geometric only. Collision response, EV, VLA, spin and batted-ball outcomes are not evaluated.',
            'Clock-invalid plans, overlap_at_start and unresolved searches are reported separately; old rows are never reconstructed.',
            'Contact percentages show both all searched and resolved-only denominators; neither is a calibrated live-game rate.',
            'Live batting, RNG and results are unchanged. This is Rust/Python parity, not independent validation of the model.'
        ])
    output.mkdir(parents=True, exist_ok=False)
    write_csv(output / (PREFIX+'_coverage.csv'), ['metric', 'count'], [dict(metric=k,count=v) for k,v in sorted(coverage.items())])
    write_csv(output / (PREFIX+'_checks.csv'), ['metric', 'max_error'], [dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    fields = ['game_id', 'count_seq', 'intent', 'selected', 'strategy', 'preferred_intent', 'decision_reason',
              'status', 'aim_z_offset_m', 'forecast_height_m', 'forecast_time_sec', 'search_iterations', 'time_offset_sec', 'contact_region']
    write_csv(output / (PREFIX+'_details.csv'), fields, details)
    summary_fields = ['scope', 'intent', 'strategy', 'preferred_intent', 'plans', 'searched', 'contact', 'no_contact',
                      'unresolved', 'overlap_at_start', 'clock_invalid', 'search_invalid', 'contact_pct_of_searched', 'contact_pct_of_resolved']
    write_csv(output / (PREFIX+'_summary.csv'), summary_fields, summaries(details))
    (output / (PREFIX+'_metadata.json')).write_text(json.dumps(metadata, indent=2)+'\n')
    return dict(status=metadata['status'], **coverage, collision_replays=0)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--first-game-id', type=int, required=True)
    parser.add_argument('--last-game-id', type=int, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(run(db_from_env(), args.first_game_id, args.last_game_id, args.output_dir), indent=2))
    except (OSError, ValueError, KeyError, TypeError, sqlite3.Error) as exc:
        parser.exit(1, f'Rust execution-contact audit failed: {exc}\n')
