from collections import Counter
import json
from pathlib import Path
import random
import tempfile
import unittest
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows
import plan_tilt_sweep as sweep
import pre_swing_aim_probe as policy
import pre_swing_aim_bootstrap as boot

FAMILY = (15., 0., 1.)
SLOW = (15., 0., .8)


def facts(fair=False, region=None, valid=True):
    return dict(evaluable=valid, geometry=fair, successful=fair, fair=fair,
                downward=region == 'downward', high_vla=region == 'high_vla', line_window=region == 'line_window')


class AimBootstrapTest(unittest.TestCase):
    def test_shared_pitch_draws_preserve_policy_and_speed_covariance(self):
        line, high = facts(True, 'line_window'), facts(True, 'high_vla')
        pitches = {(1, 1): {FAMILY: boot.vector(high, line), SLOW: boot.vector(line, high)},
                   (1, 2): {FAMILY: boot.vector(line, high), SLOW: boot.vector(high, line)}}
        families, patterns, indices = boot.compress(pitches)
        rows = list(boot.resample(patterns, indices, 50, '0'))
        for _, values in rows:
            self.assertEqual(sum(v['line_window_probability_delta_pp'] for v in values), 0)
            self.assertEqual(sum(v['high_vla_probability_delta_pp'] for v in values), 0)
        self.assertGreater(len({values[0]['line_window_probability_delta_pp'] for _, values in rows}), 1)
        self.assertEqual(rows, list(boot.resample(patterns, indices, 50, '0')))
        self.assertNotEqual(rows, list(boot.resample(patterns, indices, 50, '1')))
        # Identical policies must have zero paired difference even when outcomes vary.
        identical = {(1, 1): {FAMILY: boot.vector(line, line)}, (1, 2): {FAMILY: boot.vector(high, high)}}
        _, patterns, indices = boot.compress(identical)
        for _, values in boot.resample(patterns, indices, 50, '9'):
            self.assertTrue(all(value == 0 for metric, value in values[0].items() if metric.endswith('_delta_pp')))

    def test_joint_compression_preserves_original_pitch_sampling_mass(self):
        a = boot.vector(facts(True, 'line_window'), facts(True, 'high_vla'))
        b = boot.vector(facts(), facts())
        pitches = {(1, 1): {FAMILY: a}, (1, 2): {FAMILY: a}, (1, 3): {FAMILY: b}}
        _, patterns, indices = boot.compress(pitches)
        self.assertEqual(len(patterns), 2)
        self.assertEqual(indices, [0, 0, 1])
        expected = []
        rng = random.Random('5')
        original = [a, a, b]
        for i in range(20):
            sample = [original[rng.randrange(3)] for _ in original]
            total = [sum(v[j] for v in sample) for j in range(boot.VECTOR_SIZE)]
            expected.append((i + 1, [boot.metrics(total)]))
        self.assertEqual(expected, list(boot.resample(patterns, indices, 20, '5')))
        shuffled = dict(reversed(list(pitches.items())))
        self.assertEqual(boot.compress(pitches), boot.compress(shuffled))

    def test_conditional_denominators_invalid_pitches_and_common_fair(self):
        no, invalid = facts(), facts(valid=False)
        pitches = {(1, 1): {FAMILY: boot.vector(facts(True, 'downward'), no)},
                   (1, 2): {FAMILY: boot.vector(facts(True, 'high_vla'), facts(True, 'line_window'))},
                   (1, 3): {FAMILY: boot.vector(no, facts(True, 'downward'))},
                   (1, 4): {FAMILY: boot.vector(invalid, invalid)}}
        _, patterns, indices = boot.compress(pitches)
        point = boot.metrics(boot.aggregate(patterns, Counter(indices))[0])
        self.assertAlmostEqual(point['reference_fair_probability_per_evaluable_trial'], 2/3)
        self.assertAlmostEqual(point['candidate_fair_probability_per_evaluable_trial'], 2/3)
        self.assertEqual(point['fair_probability_delta_pp'], 0)
        self.assertEqual(point['line_window_fraction_delta_pp'], 50)
        self.assertEqual(point['common_fair_line_window_delta_pp'], 100)
        one_sided = boot.metrics(boot.aggregate(patterns, Counter({indices[2]: 4}))[0])
        self.assertEqual(one_sided['reference_fair_probability_per_evaluable_trial'], 0)
        self.assertIsNone(one_sided['reference_line_window_fraction_among_fair'])
        self.assertIsNone(one_sided['line_window_fraction_delta_pp'])
        self.assertIsNone(one_sided['common_fair_line_window_delta_pp'])
        invalid_only = boot.metrics(boot.aggregate(patterns, Counter({indices[3]: 4}))[0])
        self.assertTrue(all(v is None for v in invalid_only.values()))
        with self.assertRaisesRegex(ValueError, 'inconsistent_clock'):
            boot.vector(no, invalid)
        with self.assertRaisesRegex(ValueError, 'empty_bootstrap'):
            boot.compress({})

    def test_cached_point_reconstruction_reproducibility_and_reverse_contrast(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t); src, db = source_db(root)
            sweep.run(src, db, root / 'cache', sample_size=2, angles=(15.,), yaws=(0.,),
                      aim_z_offsets=(-.03, -.015, 0.), speed_ratios=(.8, 1.))
            config = Path(boot.__file__).with_name('pre_swing_aim_seed2_frozen.json')
            db.unlink()
            with patch('sqlite3.connect', side_effect=AssertionError('DB')), \
                 patch.object(sweep, 'evaluate', side_effect=AssertionError('search')), \
                 patch.object(sweep, 'event', side_effect=AssertionError('collision')):
                policy.run(root / 'cache', root / 'point', config)
                result = boot.run(root / 'cache', root / 'out', config, replicates=25)
                boot.run(root / 'cache', root / 'repeat', config, replicates=25)
                boot.run(root / 'cache', root / 'reverse', config,
                         reference='line_drive_low_center', candidate='contact_center', replicates=25)
            self.assertEqual(result['families'], 2)
            self.assertEqual(result['new_searches'], 0); self.assertEqual(result['db_reads'], 0)
            expected = csv_rows(root / 'point' / 'pre_swing_aim_pairs.csv')
            summary = csv_rows(root / 'out' / (boot.PREFIX + '_summary.csv'))
            for r in summary:
                if r['metric'] not in ('line_window_probability_delta_pp', 'geometry_probability_delta_pp',
                                       'fair_probability_delta_pp', 'common_fair_line_window_delta_pp'):
                    continue
                source = next(x for x in expected if x['stratum'] == 'overall'
                    and x['planned_speed_ratio'] == r['planned_speed_ratio']
                    and x['before_policy'] == 'contact_center' and x['after_policy'] == 'line_drive_low_center')
                self.assertEqual(r['point_estimate'], source[r['metric']])
                self.assertEqual(int(r['valid_replicates']) + int(r['undefined_replicates']), 25)
            self.assertEqual(sum(r['role'] == 'primary' for r in summary), 1)
            for suffix in ['summary.csv', 'replicates.csv', 'checks.csv']:
                self.assertEqual((root / 'out' / (boot.PREFIX + '_' + suffix)).read_bytes(),
                                 (root / 'repeat' / (boot.PREFIX + '_' + suffix)).read_bytes())
            reverse = csv_rows(root / 'reverse' / (boot.PREFIX + '_summary.csv'))
            for r in summary:
                if not r['metric'].endswith('_delta_pp'):
                    continue
                other = next(x for x in reverse if x['planned_speed_ratio'] == r['planned_speed_ratio'] and x['metric'] == r['metric'])
                for a, b in [('point_estimate', 'point_estimate'), ('percentile_2_5', 'percentile_97_5')]:
                    if r[a]:
                        self.assertAlmostEqual(float(r[a]), -float(other[b]))
                    else:
                        self.assertEqual(other[b], '')
            meta = json.loads((root / 'out' / (boot.PREFIX + '_metadata.json')).read_text())
            self.assertEqual(meta['resampling_unit'], 'whole_pitch_all_policies_and_families')
            self.assertEqual(meta['policies'], json.loads(config.read_text()))
            self.assertEqual(meta['inputs'][str(config)], boot.digest(config))
            self.assertTrue(meta['primary']['present'])
            checks = csv_rows(root / 'out' / (boot.PREFIX + '_checks.csv'))
            self.assertLess(max(float(r['max_error']) for r in checks), 1e-7)
            with self.assertRaises(FileExistsError):
                boot.run(root / 'cache', root / 'out', replicates=1)
            for n in [0, -1, True, 1.5]:
                with self.assertRaisesRegex(ValueError, 'invalid_bootstrap'):
                    boot.run(root / 'cache', root / 'bad', replicates=n)
            for ref, cand in [('contact_center', 'contact_center'), ('unknown', 'line_drive_low_center')]:
                with self.assertRaisesRegex(ValueError, 'two_distinct'):
                    boot.run(root / 'cache', root / 'bad', reference=ref, candidate=cand, replicates=1)
            self.assertFalse((root / 'bad').exists())


if __name__ == '__main__':
    unittest.main()
