import copy
from collections import defaultdict
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from test_observation_update_probe import input_with_history
from test_rust_intent_plan_audit import database
from test_forecast_contact_geometry import csv_rows
from batting_intent import BattedBallIntent, frozen_planner
from swing_execution import SwingPlan
from rust_intent_plan_audit import prediction_input
import observation_update_contract
import rust_observed_intent_audit as audit

FIXTURE = Path(__file__).resolve().parents[1] / 'fixtures' / 'observation_update_contract.json'


def sample(seq=1, strategy='let_batter_decide', preference='contact_first', missing=False):
    p = input_with_history(); planner = frozen_planner()
    noise = audit.standard_noise(1, seq)
    record = dict(version=1, source='observed_intent_shadow_v1', game_id=1, count_seq=seq,
        sensor_sigma_m=.005, eye=p['batting_eye'], standard_noise=noise,
        decision=audit.decide(strategy, preference), invalid_reason=None, estimate=None, intent_plans=None)
    if missing:
        p.pop('observation_history'); record['invalid_reason'] = 'missing_observation_history'
    else:
        observations = audit.observe(p['observation_history'], .005, p['batting_eye'], noise)
        f, v, a = audit.estimate(observations)
        record['estimate'] = dict(observations=observations, velocity=v, acceleration=a, forecast=f)
        inputs = prediction_input(dict(p, forecast=f))
        plans = []
        for intent in BattedBallIntent:
            built = planner.plan(inputs, intent)
            execution = SwingPlan.from_saved(built.forecast_plan)
            reason = 'execution_starts_before_plan' if execution.start_time_sec < p['observation']['time_sec'] - 1e-10 else None
            plans.append(dict(intent=intent.value, invalid_reason=reason, plan=built.record(),
                              execution_plan=execution.record() if reason is None else None))
        record['intent_plans'] = dict(version=1, source='forecast_intent_shadow_v1', input=asdict(inputs),
            settings=dict(tilt_deg=15., yaw_batter_deg=0., speed_ratio=1., duration_sec=.18, anchor_fraction=.5),
            available_time_sec=p['observation']['time_sec'], plans=plans)
    return dict(swing_prediction=dict(version=1, source='expected_pitch_prior_commit_observation_v1', invalid_reason=None, input=p),
                observed_intent_plans=record)


class ObservedIntentAuditTest(unittest.TestCase):
    def test_noise_and_estimator_fixture_matches_python(self):
        self.assertEqual(json.loads(FIXTURE.read_text()), json.loads(json.dumps(observation_update_contract.cases())))

    def test_strategy_and_player_levels_preserve_distinct_decision_sources(self):
        planner = frozen_planner()
        for strategy, preference, expected in [('let_batter_decide', 'line_drive', 'line_drive'),
            ('put_ball_in_play', 'line_drive', 'contact_first'), ('seek_line_drive', 'contact_first', 'line_drive')]:
            state = sample(strategy=strategy, preference=preference)
            rows, error = audit.compare_observed(state, 1, 1, planner, defaultdict(float))
            self.assertIsNone(error)
            selected = next(r for r in rows if r['selected'])
            self.assertEqual(selected['intent'], expected)
            self.assertEqual(selected['preferred_intent'], preference)
            self.assertEqual(selected['strategy'], strategy)
            changed = copy.deepcopy(state); changed['input'] = dict(ball_position_world_m=[99,99,99])
            self.assertEqual((rows, error), audit.compare_observed(changed, 1, 1, planner, defaultdict(float)))

    def test_invalid_update_never_falls_back_and_corruption_is_rejected(self):
        planner = frozen_planner()
        bad = sample(missing=True)
        rows, error = audit.compare_observed(bad, 1, 1, planner, defaultdict(float))
        self.assertEqual(rows, []); self.assertEqual(error, 'missing_observation_history')
        bad['observed_intent_plans']['intent_plans'] = sample()['observed_intent_plans']['intent_plans']
        with self.assertRaisesRegex(ValueError, 'fallback'):
            audit.compare_observed(bad, 1, 1, planner, defaultdict(float))
        for mutate in [lambda r: r['decision'].update(selected_intent='line_drive'),
                       lambda r: r['estimate'].update(velocity=[99,99,99]),
                       lambda r: r.update(game_id=2), lambda r: r.update(sensor_sigma_m=.01),
                       lambda r: r.update(standard_noise=[[0,0,0]]*3)]:
            state = sample(); mutate(state['observed_intent_plans'])
            with self.assertRaises(ValueError):
                audit.compare_observed(state, 1, 1, planner, defaultdict(float))

    def test_readonly_db_covers_missing_and_failed_updates(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t); db = root / 'data.db'
            old = sample(seq=2); old.pop('observed_intent_plans')
            database(db, [(1, sample(), None), (2, old, None), (3, sample(seq=3, missing=True), None)])
            before = hashlib.sha256(db.read_bytes()).hexdigest()
            result = audit.run(db, 1, 1, root / 'out')
            self.assertEqual(result['matched_forecasts'], 1)
            self.assertEqual(result['matched_plans'], 2)
            self.assertEqual(result['missing_observed_intent_states'], 1)
            self.assertEqual(result['update_invalid:missing_observation_history'], 1)
            self.assertEqual(result['selected:contact_first'], 1)
            self.assertEqual(before, hashlib.sha256(db.read_bytes()).hexdigest())
            checks = csv_rows(root / 'out' / 'rust_observed_intent_checks.csv')
            self.assertLess(max(float(r['max_error']) for r in checks), 1e-8)
            with self.assertRaises(FileExistsError):
                audit.run(db, 1, 1, root / 'out')


if __name__ == '__main__':
    unittest.main()
