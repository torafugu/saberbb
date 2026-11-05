from collections import Counter
import copy
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

import intent_selection_policy as policy
import rust_intent_collision_probe as collision
from test_rust_intent_game_bootstrap import source_fixture, rewrite_csv
from test_seeded_holdout_audit import fixture as seeded_fixture
import seeded_holdout_audit as audit


def aggregates():
    """Opposite outcomes and uneven game sizes expose selection/denominator mistakes."""
    games = {}
    for game, preference, n in [(1, 'line_drive', 100), (2, 'contact_first', 1)]:
        g = collision.Aggregate()
        for intent in ('contact_first', 'line_drive'):
            counts = Counter(plans=n, searched=n, geometry=n, successful=n)
            if intent == 'line_drive':
                counts.update(fair=n, line_window=n, fair_n_ev_ms=n, fair_sum_ev_ms=30*n)
            g.groups['all_plans', intent, 'let_batter_decide', preference, 'overall', 'all'] = counts
        games[game] = g
    return games


class IntentSelectionPolicyTest(unittest.TestCase):
    def test_coach_and_player_choices_are_separate_and_take_no_outcome_input(self):
        expected = ['contact_first', 'line_drive', 'contact_first', 'contact_first', 'line_drive', 'line_drive']
        self.assertEqual([policy.selection(s, '', '')['selected_intent'] for s in policy.SCENARIOS[1:]], expected)
        self.assertEqual(policy.selection(policy.SCENARIOS[0], 'let_batter_decide', 'line_drive')['selected_intent'], 'line_drive')
        d = policy.selection(policy.SCENARIOS[3], '', '')
        self.assertEqual(d['reason'], 'manager_contact_priority')
        self.assertEqual(policy.selection(policy.SCENARIOS[4], '', '')['preferences']['preferred_intent'], 'line_drive')
        with self.assertRaises(TypeError):
            policy.selection(policy.SCENARIOS[0], 'let_batter_decide', 'contact_first', {'fair': True})

    def test_recorded_mixed_preferences_not_replaced_by_outcome_or_majority(self):
        games = aggregates(); before = copy.deepcopy(games[1].groups)
        totals = policy.total_vectors(policy.vectors_for(games), Counter({0: 1, 1: 1}))
        self.assertEqual(totals['recorded']['plans'], 101)
        self.assertAlmostEqual(policy.metrics(totals['recorded'])['fair_pct_per_pitch'], 100*100/101)
        self.assertEqual(policy.metrics(totals['contact_first'])['fair_pct_per_pitch'], 0)
        self.assertEqual(policy.metrics(totals['line_drive'])['fair_pct_per_pitch'], 100)
        self.assertEqual(games[1].groups, before)
        # Multiplicity 2 for game 1 and 0 for game 2, rather than a mean of game rates.
        weighted = policy.total_vectors(policy.vectors_for(games), Counter({0: 2}))
        self.assertEqual(weighted['recorded']['plans'], 200)
        self.assertEqual(policy.metrics(weighted['recorded'])['fair_pct_per_pitch'], 100)

    def test_unsearched_and_unavailable_responses_remain_in_per_pitch_denominator(self):
        c = Counter(plans=10, searched=8, geometry=6, successful=4, fair=2, line_window=1)
        values = policy.metrics(c)
        self.assertEqual(values['successful_pct_per_pitch'], 40)
        self.assertEqual(values['fair_pct_per_pitch'], 20)
        self.assertEqual(values['line_window_pct_among_fair'], 50)
        self.assertIsNone(values['mean_fair_ev_ms'])
        self.assertTrue(all(v is None for v in policy.metrics(Counter()).values()))
        result = policy.interval([0.], 2, 0., .95)
        self.assertEqual(result['interval_status'], 'undefined_replicates')
        self.assertIsNone(result['ci_low'])
        self.assertEqual(policy.interval([0., 0.], 2, 0., .95)['ci_high'], 0)

    def test_cache_evaluation_equivalent_policies_and_shared_delta_intervals(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); source = source_fixture(root)
            before = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()}
            with patch('finite_barrel_collision.solve', side_effect=AssertionError('must not solve')):
                result = policy.run(source, root/'out', 30, 'fixed')
            self.assertEqual(result['scenarios'], 7)
            self.assertEqual(result['observed_games'], 2)
            rows = policy.cache.read_csv(root/'out/intent_selection_policy_summary.csv')
            def selected(name):
                return {r['metric']: (r['estimate'], r['ci_low'], r['ci_high'], r['delta_vs_contact_first'], r['delta_ci_low'], r['delta_ci_high'])
                        for r in rows if r['scenario'] == name}
            contact = selected('let_batter_decide__contact_first')
            self.assertEqual(contact, selected('put_ball_in_play__contact_first'))
            self.assertEqual(contact, selected('put_ball_in_play__line_drive'))
            self.assertEqual(selected('let_batter_decide__line_drive'), selected('seek_line_drive__contact_first'))
            self.assertEqual(selected('seek_line_drive__line_drive'), selected('seek_line_drive__contact_first'))
            for r in rows:
                if r['scenario']=='put_ball_in_play__line_drive' and r['delta_vs_contact_first']!='':
                    self.assertEqual(float(r['delta_vs_contact_first']), 0.)
                    if r['delta_interval_status']=='interval_available':self.assertEqual(float(r['delta_ci_high']), 0.)
            decisions = policy.cache.read_csv(root/'out/intent_selection_policy_decisions.csv')
            d = next(r for r in decisions if r['scenario']=='put_ball_in_play__line_drive')
            self.assertEqual(d['effective_preferred_intent'], 'line_drive')
            self.assertEqual(d['selected_intent'], 'contact_first')
            self.assertEqual(d['decision_reason'], 'manager_contact_priority')
            self.assertEqual(before, {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()})
            meta = json.loads((root/'out/intent_selection_policy_metadata.json').read_text())
            self.assertFalse(meta['policy_fitting'])
            self.assertEqual(meta['db_reads'], 0)
            self.assertEqual(meta['new_searches'], 0)
            self.assertEqual(meta['collision_replays'], 0)
            with self.assertRaises(FileExistsError):policy.run(source, root/'out', 1)

    def test_verified_seeded_population_keeps_zero_contribution_games(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); db, folder = seeded_fixture(root)
            audit.run(db, folder, root/'cache')
            result = policy.run(root/'cache', root/'out', 30, 'zero-cluster')
            self.assertEqual(result['observed_games'], 2)
            self.assertEqual(result['contributing_games'], 1)
            self.assertGreater(result['intervals_unavailable'], 0)
            rows = policy.cache.read_csv(root/'out/intent_selection_policy_summary.csv')
            self.assertTrue(all(r['ci_low']=='' for r in rows))

    def test_invalid_cache_rejected_before_creating_output_and_cli_reproducible(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); source = source_fixture(root)
            env = dict(os.environ); env.pop('SABERBB_DB', None)
            for name in ('a', 'b'):
                cmd = [sys.executable, str(ROOT/'tools/diagnostics/intent_selection_policy.py'), '--input-dir', str(source),
                       '--output-dir', str(root/name), '--replicates', '20', '--bootstrap-seed', 'fixed']
                result = subprocess.run(cmd, env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
            for suffix in ('summary.csv', 'decisions.csv', 'height.csv', 'coverage.csv', 'checks.csv', 'metadata.json'):
                self.assertEqual((root/'a'/('intent_selection_policy_'+suffix)).read_bytes(),
                                 (root/'b'/('intent_selection_policy_'+suffix)).read_bytes())
            rewrite_csv(source/'rust_intent_collision_details.csv', lambda rows: rows[0].update(fair='False' if rows[0]['fair']=='True' else 'True'))
            with self.assertRaises(ValueError):policy.run(source, root/'bad', 1)
            self.assertFalse((root/'bad').exists())


if __name__ == '__main__':
    unittest.main()
