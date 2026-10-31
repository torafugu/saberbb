"""Run with python3 tests/sql/test_contact_miss_diagnostics.py (stdlib only)."""
import re
import sqlite3
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class ContactMissDiagnosticsTest(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        ddl = (ROOT / 'migrations/ddl/player_game_batting_table.sql').read_text()
        self.db.executescript(re.sub(r'^DROP TABLE .*?;\s*', '', ddl))
        sql = (ROOT / 'sql/diagnostics/contact_miss_diagnostics.sql').read_text()
        sql = re.sub(r'^--.*$', '', sql, flags=re.M)
        self.queries = [q.strip() for q in sql.split(';') if q.strip()]
        self.params = {'first_game_id': 1, 'last_game_id': 1}

    def tearDown(self):
        self.db.close()

    def add(self, seq, length, thickness, kind='SwungAndMiss', **extra):
        columns = self.db.execute('PRAGMA table_info(player_game_batting)').fetchall()
        values = {c[1]: (0 if 'REAL' in c[2] or 'INT' in c[2] else '')
                  for c in columns if c[3] and c[4] is None}
        values.update(game_id=1, count_seq=seq, length_offset_m=length,
                      thickness_offset_m=thickness, adjusted_contact_type=kind,
                      result='Strikeout', decision_count_status='C02', decision_swung=1,
                      decision_location_x=0, decision_location_y=0,
                      adapted_timing_offset_sec=0.002, timing_impact_x_m=0.08)
        values.update(extra)
        self.db.execute('INSERT INTO player_game_batting (' + ','.join(values) + ') VALUES ('
                        + ','.join('?' for _ in values) + ')', list(values.values()))

    def query(self, index):
        cursor = self.db.execute(self.queries[index], self.params)
        names = [c[0] for c in cursor.description]
        return [dict(zip(names, row)) for row in cursor]

    def test_independent_limits_boundaries_missing_values_and_strata(self):
        self.add(1, 0.17, 0.01)
        self.add(2, 0.01, 0.071)
        self.add(3, 0.17, 0.071)
        self.add(4, 0.16, 0.070, 'MarginalContact', result='Foul')
        self.add(5, 0.01, None, decision_count_status=None,
                 decision_location_x=None, adapted_timing_offset_sec=None)
        self.add(6, 0.2, 0.08, game_id=2)  # Outside the requested cohort.
        self.add(7, 0.0, 0.0, 'Take', decision_swung=0)
        groups = {r['geometry_group']: r for r in self.query(1)}
        self.assertEqual(set(groups), {'length_only', 'thickness_only', 'both_exceeded',
                                      'within_limits', 'unknown_geometry'})
        self.assertTrue(all(r['swings'] == 1 and r['pct_of_swings'] == 20 for r in groups.values()))
        strata = self.query(2)
        self.assertEqual(sum(r['swings'] for r in strata), 5)
        self.assertTrue(any(r['timing_band'] == '01_2..<5ms' for r in strata))
        self.assertTrue(any(r['timing_band'] == 'unknown_timing' for r in strata))
        detail = self.query(3)
        self.assertEqual(len(detail), 4)
        self.assertEqual(next(r for r in detail if r['count_seq'] == 3)['geometry_group'], 'both_exceeded')
        self.assertEqual(sum(r['swings'] for r in self.query(0)), 5)

    def test_mismatches_are_visible_and_empty_ranges_are_valid(self):
        self.add(1, 0.16, 0.070)  # Miss with neither limit exceeded.
        self.add(2, 0.17, 0.01, 'SolidContact', result='Single')
        self.add(3, 0.17, 0.01, decision_swung=None, decision_count_status=None)
        reasons = {r['consistency'] for r in self.query(0)}
        self.assertIn('miss_within_limits', reasons)
        self.assertIn('contact_outside_limits', reasons)
        self.assertEqual(len(self.query(3)), 2)  # Legacy typed swing retained.
        self.params = {'first_game_id': 9, 'last_game_id': 10}
        for i in range(4):
            self.assertEqual(self.query(i), [])


if __name__ == '__main__':
    unittest.main()
