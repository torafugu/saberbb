from dataclasses import replace
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows
from batting_intent import BattedBallIntent as Intent, PlanningInput, frozen_planner, FROZEN_POLICIES
import batting_intent_probe as probe
import pre_swing_aim_probe as policy
import plan_tilt_sweep as sweep


def inputs(height=.8, side=1):
    return PlanningInput((.1, 0., height), .4, 10., 35., side, 1.)


class BattingIntentTest(unittest.TestCase):
    def test_explicit_intent_moves_aim_and_pivot_without_changing_forecast_or_velocity(self):
        planner = frozen_planner()
        for side in (-1, 1):
            for height, offset in [(.599999, 0.), (.6, -.015), (.899999, -.015), (.9, -.015), (1.4, -.015)]:
                before = planner.plan(inputs(height, side), Intent.CONTACT_FIRST)
                after = planner.plan(inputs(height, side), Intent.LINE_DRIVE)
                self.assertEqual(before.aim_policy, 'contact_center')
                self.assertEqual(after.aim_policy, 'line_drive_low_center')
                self.assertEqual(after.aim_z_offset_m, offset)
                a, b = before.forecast_plan, after.forecast_plan
                for field in ['forecast_position_world_m', 'nominal_time_sec', 'ideal_axis_world', 'actual_axis_world',
                              'ideal_velocity_world_ms', 'actual_velocity_world_ms', 'ideal_omega_world_rad_s']:
                    self.assertEqual(a[field], b[field])
                for field in ['ideal_point_world_m', 'actual_point_world_m', 'pivot_world_m']:
                    self.assertEqual(a[field][:2], b[field][:2])
                    self.assertAlmostEqual(b[field][2] - a[field][2], offset)
                self.assertEqual(after.record()['intent'], 'line_drive')
        with self.assertRaisesRegex(ValueError, 'explicit_intent'):
            planner.plan(inputs(), 'line_drive')
        with self.assertRaises(ValueError):
            planner.plan(inputs(), Intent.LINE_DRIVE, speed_ratio=0)
        with self.assertRaises(ValueError):
            planner.plan(inputs(), Intent.LINE_DRIVE, tilt_deg=float('nan'))
        with self.assertRaises(TypeError):
            PlanningInput((0, 0, .8), .4, 10, 35, 1, 0, actual_normal=[0, 0, 1])
        for bad in [float('nan'), float('inf')]:
            with self.assertRaises(ValueError):
                replace(inputs(), forecast_time_sec=bad)

    def test_saved_input_adapter_ignores_outcomes_and_previous_aim(self):
        planner = frozen_planner()
        saved = planner.plan(inputs(), Intent.CONTACT_FIRST).forecast_plan
        a = PlanningInput.from_forecast_plan(saved)
        poisoned = dict(saved, actual_point_world_m=(99, 99, 99), planned_aim_point_world_m=(99, 99, 99),
                        planned_aim_offset_world_m=(0, 0, -1), contact={'normal': [99, 99, 99]},
                        responses={'ev_ms': 99, 'vla_deg': -99})
        b = PlanningInput.from_forecast_plan(poisoned)
        self.assertEqual(a, b)
        self.assertEqual(planner.plan(a, Intent.LINE_DRIVE), planner.plan(b, Intent.LINE_DRIVE))

    def test_cli_pipeline_matches_cached_policy_results_and_supports_one_intent(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t); src, db = source_db(root)
            sweep.run(src, db, root / 'cache', sample_size=2, angles=(15.,), yaws=(0.,),
                      aim_z_offsets=(-.03, -.015, 0.), speed_ratios=(.8, 1.))
            db.unlink()
            with patch('sqlite3.connect', side_effect=AssertionError('DB')), \
                 patch.object(sweep, 'evaluate', side_effect=AssertionError('search')), \
                 patch.object(sweep, 'event', side_effect=AssertionError('collision')):
                policy.run(root / 'cache', root / 'old', FROZEN_POLICIES)
                result = probe.run(root / 'cache', root / 'new')
                probe.run(root / 'cache', root / 'single', intents=('line_drive',))
            self.assertEqual(result['generated_plans'], 8)
            self.assertEqual(result['new_searches'], 0); self.assertEqual(result['db_reads'], 0)
            expected = csv_rows(root / 'old' / 'pre_swing_aim_summary.csv')
            mapping = dict(contact_first='contact_center', line_drive='line_drive_low_center')
            for r in csv_rows(root / 'new' / 'batting_intent_summary.csv'):
                old = next(x for x in expected if x['policy'] == mapping[r['intent']]
                    and all(x[k] == r[k] for k in ['tilt_deg', 'yaw_batter_deg', 'planned_speed_ratio', 'stratum', 'forecast_height_band_m']))
                self.assertEqual({k: v for k, v in r.items() if k != 'intent'}, {k: v for k, v in old.items() if k != 'policy'})
            plans = [json.loads(line) for line in (root / 'new' / 'batting_intent_plans.jsonl').read_text().splitlines()]
            self.assertEqual(len(plans), 8)
            self.assertEqual({r['intent'] for r in plans}, {'contact_first', 'line_drive'})
            for r in plans:
                self.assertNotIn('contact', r); self.assertNotIn('responses', r)
                self.assertEqual(r['forecast_plan']['planned_aim_offset_world_m'][2], r['aim_z_offset_m'])
            checks = csv_rows(root / 'new' / 'batting_intent_checks.csv')
            self.assertTrue(any(r['metric'].startswith('intent_execution_plan') for r in checks))
            self.assertLess(max(float(r['max_error']) for r in checks), 1e-7)
            self.assertEqual(csv_rows(root / 'single' / 'batting_intent_pairs.csv'), [])
            self.assertEqual({r['intent'] for r in csv_rows(root / 'single' / 'batting_intent_summary.csv')}, {'line_drive'})
            meta = json.loads((root / 'new' / 'batting_intent_metadata.json').read_text())
            self.assertEqual(meta['intent_policies'], mapping)
            self.assertEqual(meta['inputs'][str(FROZEN_POLICIES)], probe.digest(FROZEN_POLICIES))
            with self.assertRaises(FileExistsError):
                probe.run(root / 'cache', root / 'new')
            for intents in [(), ('line_drive', 'line_drive'), ('unknown',)]:
                with self.assertRaises(ValueError):
                    probe.run(root / 'cache', root / 'bad', intents=intents)
            self.assertFalse((root / 'bad').exists())


if __name__ == '__main__':
    unittest.main()
