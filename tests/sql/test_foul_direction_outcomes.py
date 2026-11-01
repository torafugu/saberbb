"""Run: python3 tests/sql/test_foul_direction_outcomes.py"""
import re
import sqlite3
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class FoulDirectionOutcomesTest(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.row_factory = sqlite3.Row
        for table in ('batting', 'fielding'):
            ddl = (ROOT / f'migrations/ddl/player_game_{table}_table.sql').read_text()
            self.db.executescript(re.sub(r'^DROP TABLE .*?;\s*', '', ddl))
        sql = (ROOT / 'sql/diagnostics/foul_direction_outcomes.sql').read_text()
        self.queries = [q for q in re.sub(r'^--.*$', '', sql, flags=re.M).split(';') if q.strip()]

    def tearDown(self):
        self.db.close()

    def add(self, seq, launch, final, result, catch=None, outbound='InField', **extra):
        data = {c['name']: (0 if 'REAL' in c['type'] or 'INT' in c['type'] else '')
                for c in self.db.execute('pragma table_info(player_game_batting)') if c['notnull']}
        data.update(game_id=10, count_seq=seq, adjusted_contact_type='MarginalContact',
                    hla_model_version=2, final_hla_deg=launch, polar_angle=final,
                    outbound_result=outbound, result=result, first_bounce_time=3)
        data.update(extra)
        self.db.execute('insert into player_game_batting (' + ','.join(data)
                        + ') values (' + ','.join('?' for _ in data) + ')', list(data.values()))
        if catch:
            self.db.execute('''insert into player_game_fielding
                (game_id,count_seq,seq,catch_fielder_id,catch_fielder_position,time_to_field,
                 play_type,selection_path,catch_type,catch_time_sec,catch_z_m)
                values (10,?,1,1,'C',2,'CatchPlay','interception',?,2,1)''', (seq, catch))

    def query(self, index, lo=10, hi=10):
        return [dict(row) for row in self.db.execute(self.queries[index],
                                                    dict(first_game_id=lo, last_game_id=hi))]

    def test_gaps_keep_missing_rows_and_separate_direction_changes(self):
        self.add(1, 130, 120, 'Out', 'DirectFly')
        self.add(2, 120, 110, 'Foul')  # evaluated foul need not have a fielding row
        self.add(3, 100, 20, 'DoublePlay', 'BounceCatch')
        self.add(4, 10, 50, 'Foul')
        self.add(5, 10, 10, 'Single', 'FinalPickup')
        # An additional throw must not multiply a contact.
        self.db.execute('''insert into player_game_fielding
            (game_id,count_seq,seq,catch_fielder_id,catch_fielder_position,time_to_field,play_type)
            values (10,1,2,2,'SS',4,'TouchPlay')''')
        row = self.query(0)[0]
        self.assertEqual(row['contacts'], 5)
        self.assertEqual(row['launch_foul_non_foul_results'], 2)
        self.assertEqual(row['launch_fair_foul_results'], 1)
        self.assertEqual(row['missing_fielding_rows'], 2)
        self.assertEqual(sum(r['contacts'] for r in self.query(1)), 5)
        self.assertEqual(sum(r['contacts'] for r in self.query(2)), 3)
        details = {r['count_seq']: r for r in self.query(3)}
        self.assertEqual(details[1]['resolution_hint'], 'direct_fly_before_final_foul_check')
        self.assertEqual(details[1]['fly_boundary_status'], 'before_recorded_boundaries')
        self.assertEqual(details[3]['final_direction'], 'fair')
        self.assertEqual(details[3]['catch_type'], 'BounceCatch')
        self.assertEqual(details[4]['fielding_coverage'], 'missing_row')
        for i in range(4):
            self.assertEqual(self.query(i, 11, 12), [])

    def test_boundaries_legacy_missing_diagnostics_and_early_return(self):
        self.add(1, -45, -90, 'Out', 'DirectFly', first_bounce_time=2)
        self.add(2, 90, 30, 'HomeRun', outbound='HomeRun', hla_model_version=None)
        self.add(3, 90, 80, 'Out', 'DirectFly', first_bounce_time=4, fence_impact_time=1.5)
        self.db.execute('update player_game_fielding set catch_type=NULL where count_seq=3')
        details = {r['count_seq']: r for r in self.query(3)}
        self.assertEqual(details[1]['launch_direction'], 'foul_side')
        self.assertEqual(details[1]['final_direction'], 'backward')
        self.assertEqual(details[1]['fly_boundary_status'], 'at_or_after_event_boundary')
        self.assertEqual(details[2]['hla_version'], 1)
        self.assertEqual(details[2]['resolution_hint'], 'outbound_home_run')
        self.assertEqual(details[3]['fielding_coverage'], 'missing_catch_diagnostics')
        self.db.execute("update player_game_fielding set catch_type='DirectFly' where count_seq=3")
        self.assertEqual({r['count_seq']: r for r in self.query(3)}[3]['fly_boundary_status'],
                         'at_or_after_event_boundary')

    def test_details_limit_does_not_truncate_aggregate(self):
        for i in range(201):
            self.add(i, 100, 100, 'Out', 'DirectFly')
        self.assertEqual(self.query(0)[0]['contacts'], 201)
        self.assertEqual(sum(r['contacts'] for r in self.query(2)), 201)
        self.assertEqual(len(self.query(3)), 200)


if __name__ == '__main__':
    unittest.main()
