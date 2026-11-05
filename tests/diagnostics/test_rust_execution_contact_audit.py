import copy
from collections import defaultdict
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/diagnostics'))

from batting_intent import frozen_planner
from diagnostic_db import db_from_env
from intent_execution_contract import cases, evaluate, pose_samples, SETTINGS
from swing_execution import SwingPlan
from test_rust_observed_intent_audit import sample as observed_sample
from test_rust_intent_plan_audit import database
from test_forecast_contact_geometry import csv_rows
from rust_observed_intent_audit import standard_noise, observe, estimate
import rust_execution_contact_audit as audit

FIXTURE = ROOT / 'tests/fixtures/intent_execution_contract.json'


def sample(seq=1, strategy='let_batter_decide', preference='contact_first', clock_invalid=False):
    state = observed_sample(seq, strategy, preference)
    state['input'] = json.loads(FIXTURE.read_text())['cases'][0]['truth']
    observed = state['observed_intent_plans']
    if clock_invalid:
        # Move the saved history toward the plate; the forecast now requires a
        # swing start before the last observation used to make the plan.
        p = state['swing_prediction']['input']
        for point in p['observation_history']:
            point['position_world_m'][1] -= 4.
        p['observation']['position_world_m'][1] -= 4.
        noise = standard_noise(1, seq)
        observations = observe(p['observation_history'], .005, p['batting_eye'], noise)
        forecast, velocity, acceleration = estimate(observations)
        observed['estimate'] = dict(observations=observations, velocity=velocity, acceleration=acceleration, forecast=forecast)
        from rust_intent_plan_audit import prediction_input
        from batting_intent import BattedBallIntent
        from dataclasses import asdict
        inputs = prediction_input(dict(p, forecast=forecast))
        observed['intent_plans']['input'] = asdict(inputs)
        for plan in observed['intent_plans']['plans']:
            planned = frozen_planner().plan(inputs, BattedBallIntent(plan['intent']))
            plan.update(plan=planned.record(), invalid_reason='execution_starts_before_plan', execution_plan=None)
    probes = []
    for plan in observed['intent_plans']['plans']:
        probe = dict(intent=plan['intent'], selected=plan['intent'] == observed['decision']['selected_intent'],
                     invalid_reason=plan['invalid_reason'], samples=[], search=None)
        if plan['execution_plan']:
            execution = SwingPlan.from_saved(plan['plan']['forecast_plan'])
            samples, result = evaluate(state['input'], execution)
            probe.update(samples=samples, search=result)
        probes.append(probe)
    state['execution_contact'] = dict(version=1, source='intent_execution_contact_shadow_v1',
        decision=copy.deepcopy(observed['decision']), settings=dict(SETTINGS), probes=probes)
    return state


class ExecutionContactAuditTest(unittest.TestCase):
    def test_python_fixture_covers_search_states_and_caps(self):
        fixture = json.loads(FIXTURE.read_text())
        self.assertEqual(fixture, json.loads(json.dumps(cases())))
        self.assertEqual({c['search']['status'] for c in fixture['cases']}, {'contact', 'no_contact', 'overlap_at_start', 'unresolved'})
        self.assertIn('cap', {c['search']['geometry']['contact_region'] for c in fixture['cases'] if c['search']['geometry']})

    def test_coach_player_disagreement_and_no_truth_feedback_to_planning(self):
        for strategy, preference, selected in [('let_batter_decide', 'line_drive', 'line_drive'),
            ('put_ball_in_play', 'line_drive', 'contact_first'), ('seek_line_drive', 'contact_first', 'line_drive')]:
            state = sample(strategy=strategy, preference=preference)
            rows, error = audit.compare_execution(state, 1, 1, frozen_planner(), defaultdict(float))
            self.assertIsNone(error)
            self.assertEqual([r['intent'] for r in rows if r['selected']], [selected])
            before = copy.deepcopy(state['observed_intent_plans'])
            state['input']['ball_position_world_m'] = [99., 0., 99.]
            for probe, plan in zip(state['execution_contact']['probes'], before['intent_plans']['plans']):
                execution = SwingPlan.from_saved(plan['plan']['forecast_plan'])
                samples, result = evaluate(state['input'], execution)
                probe.update(samples=samples, search=result)
            changed, _ = audit.compare_execution(state, 1, 1, frozen_planner(), defaultdict(float))
            self.assertEqual(state['observed_intent_plans'], before)
            self.assertTrue(all(r['status'] == 'no_contact' for r in changed))

    def test_corrupt_selection_pose_search_and_clock_fallback_are_rejected(self):
        mutations = [lambda p: p.update(selected=not p['selected']),
            lambda p: p['samples'][0].update(point_world_m=[99., 0., 0.]),
            lambda p: p['search'].update(iterations=99999),
            lambda p: p['search'].update(status='unresolved')]
        for mutate in mutations:
            state = sample(); mutate(state['execution_contact']['probes'][0])
            with self.assertRaises(ValueError):
                audit.compare_execution(state, 1, 1, frozen_planner(), defaultdict(float))
        state = sample(clock_invalid=True)
        rows, error = audit.compare_execution(state, 1, 1, frozen_planner(), defaultdict(float))
        self.assertIsNone(error)
        self.assertTrue(all(r['status'] == 'execution_starts_before_plan' for r in rows))
        state['execution_contact']['probes'][0]['search'] = sample()['execution_contact']['probes'][0]['search']
        with self.assertRaises(ValueError):
            audit.compare_execution(state, 1, 1, frozen_planner(), defaultdict(float))

    def test_invalid_search_preserves_pose_but_never_reports_contact(self):
        state = sample(); state['input']['reference_time_sec_since_release'] = .01
        for probe, plan in zip(state['execution_contact']['probes'], state['observed_intent_plans']['intent_plans']['plans']):
            execution = SwingPlan.from_saved(plan['plan']['forecast_plan'])
            probe.update(invalid_reason='invalid_contact_search_input', samples=pose_samples(state['input'], execution), search=None)
        rows, error = audit.compare_execution(state, 1, 1, frozen_planner(), defaultdict(float))
        self.assertIsNone(error)
        self.assertTrue(all(r['status'] == 'invalid_contact_search_input' for r in rows))

    def test_readonly_pipeline_environment_cli_and_coverage(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); db = root/'data with space.db'
            old = observed_sample(seq=3)
            database(db, [(1, sample(), None), (2, sample(seq=2, clock_invalid=True), None), (3, old, None)])
            before = hashlib.sha256(db.read_bytes()).hexdigest()
            with patch.dict(os.environ, {'SABERBB_DB': str(db)}):
                self.assertEqual(db_from_env(), db)
                result = audit.run(db_from_env(), 1, 1, root/'out')
            self.assertEqual(result['matched_plans'], 4)
            self.assertEqual(result['replayed_searches'], 2)
            self.assertEqual(result['missing_execution_contact_states'], 1)
            self.assertEqual(before, hashlib.sha256(db.read_bytes()).hexdigest())
            summary = csv_rows(root/'out/rust_execution_contact_summary.csv')
            self.assertEqual(sum(int(r['plans']) for r in summary if r['scope']=='selected_plans'), 2)
            command = [sys.executable, str(ROOT/'tools/diagnostics/rust_execution_contact_audit.py'),
                '--first-game-id', '1', '--last-game-id', '1', '--output-dir', str(root/'cli')]
            completed = subprocess.run(command, env=dict(os.environ,SABERBB_DB=str(db)), capture_output=True, text=True)
            self.assertEqual(completed.returncode, 0, completed.stderr)
            failed = subprocess.run(command, env=dict(os.environ,SABERBB_DB=''), capture_output=True, text=True)
            self.assertNotEqual(failed.returncode, 0)
            self.assertIn('SABERBB_DB is not set', failed.stderr)
            with self.assertRaises(FileExistsError):
                audit.run(db, 1, 1, root/'out')


if __name__ == '__main__':
    unittest.main()
