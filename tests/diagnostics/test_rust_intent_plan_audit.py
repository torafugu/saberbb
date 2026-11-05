import copy
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

from test_forecast_contact_geometry import csv_rows
import intent_plan_contract
import rust_intent_plan_audit as audit

FIXTURE = Path(__file__).resolve().parents[1] / 'fixtures' / 'intent_plan_contract.json'


def shadow_state(available=.24, index=6):
    cases = json.loads(FIXTURE.read_text())['cases']
    first = next(c for c in cases if c['name'] == f'default_{index}_contact_first')
    inp = first['input']
    prediction = dict(forecast=dict(position_world_m=inp['forecast_position_world_m'], time_sec=inp['forecast_time_sec']),
        planned_attack_angle_deg=inp['attack_angle_deg'], bat_speed_ms=inp['bat_speed_ms'], handedness_sign=inp['handedness_sign'],
        execution_angle_error_deg=inp['execution_angle_error_deg'], observation=dict(time_sec=available), residual_execution_delay_sec=0.)
    plans = []
    for intent in ['contact_first', 'line_drive']:
        case = next(c for c in cases if c['name'] == f'default_{index}_{intent}')
        execution = case['expected_execution']
        reason = case['execution_invalid_reason']
        if reason is None and execution['start_time_sec'] < available - 1e-10:
            reason = 'execution_starts_before_plan'; execution = None
        plans.append(dict(intent=intent, invalid_reason=reason, plan=case['expected_plan'], execution_plan=execution))
    return dict(swing_prediction=dict(version=1, source='expected_pitch_prior_commit_observation_v1',
                                     invalid_reason=None, input=prediction),
                intent_plans=dict(version=1, source='forecast_intent_shadow_v1', input=inp,
                                  settings=first['settings'], available_time_sec=available, plans=plans))


def database(path, states):
    db = sqlite3.connect(path)
    db.execute('CREATE TABLE player_game_contact_state(game_id,count_seq,state_version,state_source,invalid_reason,state_json)')
    for seq, state, invalid in states:
        db.execute('INSERT INTO player_game_contact_state VALUES(1,?,1,?,?,?)',
                   (seq, audit.SOURCE, invalid, json.dumps(state) if state is not None else None))
    db.commit(); db.close()


class RustIntentAuditTest(unittest.TestCase):
    def test_shared_contract_matches_current_python_planner(self):
        self.assertEqual(json.loads(FIXTURE.read_text()), json.loads(json.dumps(intent_plan_contract.cases())))

    def test_readonly_db_pipeline_preserves_old_and_clock_invalid_rows(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t); db = root / 'test.db'
            old = shadow_state(); old.pop('intent_plans')
            failed = dict(swing_prediction=dict(invalid_reason='invalid_observation_clock', input=None))
            database(db, [(1, shadow_state(), None), (2, shadow_state(.35), None),
                          (3, old, None), (4, failed, None), (5, None, 'invalid_input')])
            before = hashlib.sha256(db.read_bytes()).hexdigest()
            result = audit.run(db, 1, 1, root / 'out')
            self.assertEqual(result['states'], 5)
            self.assertEqual(result['matched_states'], 2)
            self.assertEqual(result['matched_plans'], 4)
            self.assertEqual(result['matched_execution_plans'], 2)
            self.assertEqual(result['matched_clock_invalid_plans'], 2)
            self.assertEqual(result['missing_intent_plan_states'], 1)
            self.assertEqual(result['prediction_invalid_states'], 1)
            self.assertEqual(result['outer_invalid_states'], 1)
            self.assertEqual(before, hashlib.sha256(db.read_bytes()).hexdigest())
            checks = csv_rows(root / 'out' / 'rust_intent_plan_checks.csv')
            self.assertLess(max(float(r['max_error']) for r in checks), 1e-8)
            details = csv_rows(root / 'out' / 'rust_intent_plan_details.csv')
            self.assertEqual(len(details), 4)
            meta = json.loads((root / 'out' / 'rust_intent_plan_metadata.json').read_text())
            self.assertEqual(meta['db_access'], 'read_only_transaction')
            with self.assertRaises(FileExistsError):
                audit.run(db, 1, 1, root / 'out')
            with self.assertRaisesRegex(ValueError, 'no_contact_state'):
                audit.run(db, 2, 2, root / 'none')
            self.assertFalse((root / 'none').exists())

    def test_missing_old_records_are_not_reported_as_matching_plans(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t); db = root / 'old.db'
            old = shadow_state(); old.pop('intent_plans')
            database(db, [(1, old, None)])
            result = audit.run(db, 1, 1, root / 'out')
            self.assertEqual(result['status'], 'no_comparable_shadow_plans')
            self.assertEqual(csv_rows(root / 'out' / 'rust_intent_plan_details.csv'), [])

    def test_corruption_is_rejected_and_truth_does_not_enter_planning(self):
        planner = audit.frozen_planner()
        state = shadow_state()
        changed = copy.deepcopy(state)
        changed.update(input=dict(ball_position_world_m=[99, 99, 99]), responses=dict(ev_ms=99, vla_deg=99))
        self.assertEqual(audit.compare_shadow(state, planner, defaultdict(float)), audit.compare_shadow(changed, planner, defaultdict(float)))
        for mutate in [lambda s: s['intent_plans']['input'].update(forecast_time_sec=.5),
                       lambda s: s['intent_plans']['plans'].pop(),
                       lambda s: s['intent_plans']['plans'][0]['plan']['forecast_plan'].update(pivot_world_m=[99, 99, 99]),
                       lambda s: s['intent_plans']['plans'][0].update(invalid_reason='execution_starts_before_plan'),
                       lambda s: s['intent_plans'].update(available_time_sec=.35)]:
            bad = copy.deepcopy(state); mutate(bad)
            with self.assertRaises(ValueError):
                audit.compare_shadow(bad, planner, defaultdict(float))
        # Release-clock rejection is also compared without inventing an execution plan.
        early = shadow_state(available=.01, index=9)
        result = audit.compare_shadow(early, planner, defaultdict(float))
        self.assertTrue(all(r['status'] == 'execution_starts_before_release' for r in result))


if __name__ == '__main__':
    unittest.main()
