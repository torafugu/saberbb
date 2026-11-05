import copy
import json
import tempfile
import unittest
from collections import Counter
from pathlib import Path
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows
from pre_swing_aim import Forecast, read_policies
import pre_swing_aim_probe as probe
import plan_tilt_sweep as sweep

TOOLS = Path(__file__).resolve().parents[2] / 'tools' / 'diagnostics'


def config():
    return json.loads((TOOLS / 'pre_swing_aim_policies.json').read_text())


class PreSwingAimTest(unittest.TestCase):
    def test_forecast_boundary_rules_and_no_outcome_input(self):
        policies = read_policies(config())
        line = next(p for p in policies if p.name == 'line_drive_height')
        lift = next(p for p in policies if p.name == 'lift_height')
        for height, expected, lifted in [(0.59999, -30, -30), (.6, -15, -30),
                                          (.89999, -15, -30), (.9, 0, -15), (1.5, 0, -15)]:
            forecast = Forecast(height)
            self.assertEqual(line.choose(forecast), expected)
            self.assertEqual(lift.choose(forecast), lifted)
            cells = {-30: 'deep', -15: 'moderate', 0: 'center'}
            self.assertEqual(probe.select(forecast, policies, cells)[line.name], cells[expected])
        # Forecast is the entire policy-facing snapshot; outcomes cannot be supplied.
        with self.assertRaises(TypeError):
            Forecast(height_m=.6, actual_normal=[0, 0, 1])
        for bad in [float('nan'), float('inf'), True, '0.6']:
            with self.assertRaises(ValueError):
                Forecast(bad)
        for mutate in [lambda c: c.update(height_edges_m=[.9, .6]),
                       lambda c: c['policies']['line_drive_height'].update(aim_z_mm=[0]),
                       lambda c: c['policies']['line_drive_height'].update(actual_normal=1),
                       lambda c: c['policies']['line_drive_height'].update(aim_z_mm=[0, True, 0])]:
            c = config(); mutate(c)
            with self.assertRaises(ValueError):
                read_policies(c)

    def test_evaluation_keeps_invalid_and_unresolved_separate(self):
        invalid = probe.outcome(dict(invalid_reason='execution_starts_before_plan'), 'm')
        self.assertEqual(invalid['clock_invalid'], 1)
        self.assertEqual(invalid['evaluable'], 0)
        self.assertEqual(invalid['no_contact'], 0)
        unresolved = probe.outcome(dict(search_status='unresolved',
            responses={'m': dict(collision_status='not_evaluated:unresolved')},
            kinematics=dict(pulse_min_pivot_z_m=0)), 'm')
        self.assertEqual(unresolved['evaluable'], 1)
        self.assertEqual(unresolved['unresolved'], 1)
        self.assertEqual(unresolved['no_contact'], 0)
        # Gained/lost fair contacts can change the marginal rate with no common-fair sample.
        a = dict(invalid, evaluable=True, clock_invalid=False, fair=True, geometry=True,
                 successful=True, downward=False, high_vla=True, line_window=False, ev_ms=30, vla_deg=60)
        b = dict(unresolved)
        g = Counter(); probe.add_pair(g, a, b); probe.add_pair(g, b, a)
        self.assertEqual(g['evaluable'], 2)
        self.assertEqual(g['fair_gained'], 1); self.assertEqual(g['fair_lost'], 1)
        self.assertEqual(g['common_fair'], 0)
        self.assertEqual(g['after_high_vla'] - g['before_high_vla'], 0)
        g = Counter(); probe.add_pair(g, a, dict(a, high_vla=False, vla_deg=20))
        self.assertEqual(g['common_fair_high_vla_delta'], -1)

    def test_cached_pipeline_matches_fixed_controls_and_is_readonly(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t); src, db = source_db(root)
            sweep.run(src, db, root / 'cache', sample_size=2, angles=(15.,), yaws=(0.,),
                      aim_z_offsets=(-.03, -.015, 0.), speed_ratios=(.8, 1.))
            db.unlink()
            records = [json.loads(x) for x in (root / 'cache' / 'plan_tilt_states.jsonl').read_text().splitlines()]
            with patch('sqlite3.connect', side_effect=AssertionError('DB read')), \
                 patch.object(sweep, 'evaluate', side_effect=AssertionError('search')), \
                 patch.object(sweep, 'event', side_effect=AssertionError('collision solve')):
                result = probe.run(root / 'cache', root / 'out')
            self.assertEqual(result['policies'], 5); self.assertEqual(result['families'], 2)
            self.assertEqual(result['new_searches'], 0)
            summary = csv_rows(root / 'out' / 'pre_swing_aim_summary.csv')
            overall = [r for r in summary if r['stratum'] == 'overall']
            self.assertEqual(len(overall), 10)
            source_summary = csv_rows(root / 'cache' / 'plan_tilt_summary.csv')
            decisions = [json.loads(x) for x in (root / 'out' / 'pre_swing_aim_decisions.jsonl').read_text().splitlines()]
            self.assertEqual(len(decisions), 20)
            self.assertTrue(all('vla_deg' not in d and 'ev_ms' not in d for d in decisions))
            for row in overall:
                speed = float(row['planned_speed_ratio'])
                chosen = [d for d in decisions if d['policy'] == row['policy'] and d['planned_speed_ratio'] == speed]
                facts = [probe.outcome(next(r for r in records if r['scenario'] == d['selected_scenario']
                    and (r['game_id'], r['count_seq']) == (d['game_id'], d['count_seq'])), probe.planned.engine.DEFAULT_MODEL) for d in chosen]
                for field in probe.COUNTS:
                    self.assertEqual(int(row[field]), sum(f[field] for f in facts))
                    strata = [r for r in summary if r['policy'] == row['policy'] and
                              r['planned_speed_ratio'] == row['planned_speed_ratio'] and r['stratum'] == 'forecast_height']
                    self.assertEqual(sum(int(r[field]) for r in strata), int(row[field]))
                if row['policy'] == 'contact_center':
                    self.assertTrue(all(d['aim_z_mm'] == 0 for d in chosen))
                if row['policy'] == 'fixed_under15':
                    self.assertTrue(all(d['aim_z_mm'] == -15 for d in chosen))
                if row['policy'] == 'fixed_under30':
                    self.assertTrue(all(d['aim_z_mm'] == -30 for d in chosen))
                if row['policy'] in ('contact_center', 'fixed_under15', 'fixed_under30'):
                    # Independently generated sweep totals are the control oracle.
                    control = next(r for r in source_summary if r['scenario'] == chosen[0]['selected_scenario']
                        and r['orientation'] == 'planned_tilt' and r['scope'] == 'all_swings'
                        and r['stratum'] == 'overall' and r['model'] == probe.planned.engine.DEFAULT_MODEL)
                    for field, original in dict(geometry='geometry_contacts', successful='successful_contacts',
                        fair='fair_contacts', downward='fair_downward', high_vla='fair_high_vla',
                        evaluable='evaluable_trials', clock_invalid='execution_clock_invalid_trials').items():
                        self.assertEqual(int(row[field]), int(control[original]))
            pairs = csv_rows(root / 'out' / 'pre_swing_aim_pairs.csv')
            self.assertEqual(len([r for r in pairs if r['stratum'] == 'overall']), 20)
            for r in pairs:
                for event in probe.EVENTS:
                    self.assertAlmostEqual(float(r[event + '_probability_delta_pp']),
                        100 * (int(r['after_' + event]) - int(r['before_' + event])) / int(r['evaluable']))
                if int(r['common_fair']) == 0:
                    self.assertEqual(r['common_fair_high_vla_delta_pp'], '')
            with self.assertRaises(FileExistsError):
                probe.run(root / 'cache', root / 'out')
            # Missing even an unvisited branch is an error, never nearest-cell substitution.
            c = config(); c['policies']['line_drive_height']['aim_z_mm'][2] = -25
            path = root / 'bad.json'; path.write_text(json.dumps(c))
            with self.assertRaisesRegex(ValueError, 'missing_policy_aim_cell'):
                probe.run(root / 'cache', root / 'bad', path)
            self.assertFalse((root / 'bad').exists())

    def test_choices_survive_arbitrary_post_contact_changes(self):
        policies = read_policies(config())
        records = {'saved_tilt': dict(forecast_plan=dict(forecast_position_world_m=[0, 0, .7]))}
        scenarios = {}
        for aim in [-.03, -.015, 0.]:
            name = str(aim)
            scenarios[name] = dict(tilt_deg=15, yaw_batter_deg=0, planned_speed_ratio=1, aim_z_offset_m=aim)
            records[name] = dict(contact={'normal': [0, 0, 1]}, responses={'ev_ms': 50, 'vla_deg': 70})
        def decide(rs):
            forecast = Forecast(rs['saved_tilt']['forecast_plan']['forecast_position_world_m'][2])
            return {family: probe.select(forecast, policies, cells)
                    for family, cells in probe.family_cells(rs, scenarios).items()}
        expected = decide(records)
        changed = copy.deepcopy(records)
        for name in scenarios:
            changed[name] = dict(contact=None, responses={'ev_ms': 0, 'vla_deg': -70}, invalid_reason='anything')
        self.assertEqual(decide(changed), expected)


if __name__ == '__main__':
    unittest.main()
