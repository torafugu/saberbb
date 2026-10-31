"""Run: python3 tests/sql/test_timing_contact_probe.py"""
import math
import re
import sqlite3
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class TimingProbeTest(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        ddl = (ROOT / 'migrations/ddl/player_game_batting_table.sql').read_text()
        self.db.executescript(re.sub(r'^DROP TABLE .*?;\s*', '', ddl))
        sql = (ROOT / 'sql/diagnostics/timing_contact_probe.sql').read_text()
        self.queries = [q for q in re.sub(r'^--.*$', '', sql, flags=re.M).split(';') if q.strip()]
        self.params = {'first_game_id': 1, 'last_game_id': 1}

    def tearDown(self):
        self.db.close()

    def add(self, seq, spatial_x, timing_x, angle=30, **extra):
        x, z = spatial_x + timing_x, 0
        rad = math.radians(angle)
        length, thickness = abs(x*math.cos(rad)), abs(-x*math.sin(rad))
        contact = length <= 0.16 and thickness <= 0.070
        values = {c[1]: (0 if 'REAL' in c[2] or 'INT' in c[2] else '')
                  for c in self.db.execute('PRAGMA table_info(player_game_batting)') if c[3] and c[4] is None}
        values.update(game_id=1, count_seq=seq, actual_bat_angle_deg=angle,
                      contact_offset_x_m=x, contact_offset_z_m=z, timing_impact_x_m=timing_x,
                      length_offset_m=length, thickness_offset_m=thickness,
                      adjusted_contact_type='SolidContact' if contact else 'SwungAndMiss')
        values.update(extra)
        self.db.execute('INSERT INTO player_game_batting ('+','.join(values)+') VALUES ('
                        +','.join('?' for _ in values)+')', list(values.values()))

    def query(self, n):
        c = self.db.execute(self.queries[n], self.params)
        return [dict(zip([v[0] for v in c.description], row)) for row in c]

    def test_replay_and_recovery_use_identical_cohorts_and_no_writes(self):
        self.add(1, 0, 0.2)  # Both exceeded at 100%; contact at 50%.
        self.add(2, 0, 0.5, actual_bat_angle_deg=None)  # Legacy excluded.
        self.add(3, 0, 0.2, game_id=2)  # Other cohort.
        before = self.db.total_changes
        coverage = self.query(0)[0]
        self.assertEqual((coverage['recorded_swings'], coverage['comparable_swings'],
                          coverage['missing_input_swings'], coverage['baseline_mismatches']), (2, 1, 1, 0))
        rows = {r['timing_scale']: r for r in self.query(1)}
        self.assertEqual([rows[s]['swings'] for s in [1,.75,.5,0]], [1]*4)
        self.assertEqual(rows[1]['possible_contacts'], 0)
        self.assertEqual(rows[.5]['recovered_contacts'], 1)
        self.assertEqual(rows[0]['possible_contacts'], 1)
        self.assertEqual(len(self.query(2)), 4)
        self.assertEqual(len(self.query(3)), 4)
        self.assertEqual(self.db.total_changes, before)

    def test_cancellation_losses_and_baseline_mismatch_are_visible(self):
        self.add(1, 0.3, -0.3)
        rows = {r['timing_scale']: r for r in self.query(1)}
        self.assertEqual(rows[1]['possible_contacts'], 1)
        self.assertEqual(rows[0]['lost_contacts'], 1)
        self.db.execute('UPDATE player_game_batting SET thickness_offset_m=99')
        self.assertEqual(self.query(0)[0]['baseline_mismatches'], 1)

    def test_trial_comparison_queries_use_contact_and_pa_denominators(self):
        self.add(1, 0, 0.3, result='StrikeSwung', timing_position_scale=0.75)
        self.add(2, 0, 0, result='Single', launch_speed=40, launch_angle=15, timing_position_scale=0.75)
        sql = (ROOT / 'sql/diagnostics/compare_timing_position_trial.sql').read_text()
        queries = [q for q in re.sub(r'^--.*$', '', sql, flags=re.M).split(';') if q.strip()]
        params = dict(before_first_game_id=1, before_last_game_id=1,
                      after_first_game_id=2, after_last_game_id=2)
        cursor = self.db.execute(queries[0], params)
        row = dict(zip([c[0] for c in cursor.description], cursor.fetchone()))
        self.assertEqual(row['plate_appearances'], 1)
        self.assertEqual(row['contact_pct'], 50)
        self.assertEqual(row['avg_ev_ms'], 40)
        self.assertEqual(row['applied_scales'], '0.75')
        self.assertEqual(len(self.db.execute(queries[1], params).fetchall()), 1)
        self.assertEqual(len(self.db.execute(queries[2], params).fetchall()), 1)

    def test_scaled_record_replays_at_its_applied_scale(self):
        self.add(1, 0, 0.15, timing_position_scale=0.75)
        self.assertEqual(self.query(0)[0]['baseline_mismatches'], 0)
        rows = {r['timing_scale']: r for r in self.query(1)}
        self.assertEqual(rows[0.75]['possible_contacts'], 0)
        self.assertEqual(rows[0.5]['possible_contacts'], 1)

    def test_repository_insert_includes_angle_as_parameter_52(self):
        source = (ROOT / 'src/repositories/sql_helper/game_stat_helper.rs').read_text()
        sql = re.search(r'const INSERT_PLAYER_GAME_BATTING_SQL: &str = "(.*?)";', source, re.S)[1]
        columns = [c.strip() for c in sql.split('(', 1)[1].split(')')[0].split(',')]
        self.assertEqual(len(columns), 52)
        values = dict.fromkeys(columns, 0)
        values.update(game_id=1, count_seq=1, actual_bat_angle_deg=34.25, timing_position_scale=0.75)
        self.db.execute(sql, list(values.values()))
        self.assertEqual(self.db.execute('SELECT actual_bat_angle_deg FROM player_game_batting').fetchone(), (34.25,))

    def test_empty_cohort_and_migration_keep_unknown_angles(self):
        for i in range(4):
            rows = self.query(i)
            if i == 0:
                self.assertEqual(rows[0]['comparable_swings'], 0)
            else:
                self.assertEqual(rows, [])
        old = sqlite3.connect(':memory:')
        ddl = (ROOT / 'migrations/ddl/player_game_batting_table.sql').read_text()
        ddl = re.sub(r'^DROP TABLE .*?;\s*', '', ddl).replace('    actual_bat_angle_deg REAL,\n', '').replace('    timing_position_scale REAL CHECK (timing_position_scale > 0),\n', '')
        old.executescript(ddl)
        old.executescript((ROOT / 'migrations/upgrades/add_contact_bat_angle.sql').read_text())
        old.executescript((ROOT / 'migrations/upgrades/add_timing_position_scale.sql').read_text())
        self.assertEqual(old.execute('PRAGMA table_info(player_game_batting)').fetchall(),
                         self.db.execute('PRAGMA table_info(player_game_batting)').fetchall())
        old.close()


if __name__ == '__main__':
    unittest.main()
