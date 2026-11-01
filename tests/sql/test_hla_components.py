"""Run: python3 tests/sql/test_hla_components.py"""
import math
import re
import sqlite3
import unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]


class HlaComponentsTest(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        ddl = (ROOT/'migrations/ddl/player_game_batting_table.sql').read_text()
        self.db.executescript(re.sub(r'^DROP TABLE .*?;\s*', '', ddl))
        sql = (ROOT/'sql/diagnostics/hla_components.sql').read_text()
        self.queries = [q for q in re.sub(r'^--.*$', '', sql, flags=re.M).split(';') if q.strip()]

    def tearDown(self):
        self.db.close()

    def add(self, seq, kind, sign, phase_deg, raw_time, x):
        data = {c[1]: (0 if 'REAL' in c[2] or 'INT' in c[2] else '')
                for c in self.db.execute('pragma table_info(player_game_batting)') if c[3] and c[4] is None}
        omega = 30.0
        replay = ((2-3+phase_deg)*.85+math.degrees(math.asin(max(-1,min(1,x/.07))))*.25)
        data.update(game_id=1,count_seq=seq,adjusted_contact_type=kind,hla_handedness_sign=sign,
                    approach_variation_deg=2,pull_bias_deg=-3,timing_angle_error=phase_deg,
                    angular_velocity=omega,adapted_timing_offset_sec=raw_time,
                    contact_offset_x_m=x,final_hla_deg=replay*(sign or 1),result='Foul')
        self.db.execute('insert into player_game_batting ('+','.join(data)+') values ('
                        +','.join('?' for _ in data)+')',list(data.values()))

    def query(self, i):
        c=self.db.execute(self.queries[i],dict(first_game_id=1,last_game_id=1))
        return [dict(zip([x[0] for x in c.description],r)) for r in c]

    def test_comparison_queries_run_on_both_versions(self):
        self.add(1,'SolidContact',1,0,0,0)
        self.add(2,'WeakContact',1,0,0,0)
        self.db.execute('UPDATE player_game_batting SET game_id=2,hla_model_version=2 WHERE count_seq=2')
        sql=(ROOT/'sql/diagnostics/compare_contact_normal_hla.sql').read_text()
        queries=[q for q in re.sub(r'^--.*$', '', sql, flags=re.M).split(';') if q.strip()]
        params=dict(before_first_game_id=1,before_last_game_id=1,after_first_game_id=2,after_last_game_id=2)
        for q in queries:
            self.assertEqual(len(self.db.execute(q,params).fetchall()),2)

    def test_version_two_replay_uses_signed_local_geometry_and_wraps(self):
        for seq, sign in [(1,1),(2,-1)]:
            self.add(seq,'WeakContact',sign,10,.001,0.01)
            angle=30.0
            x,z=0.01,0.06
            radial=(-x*math.sin(math.radians(angle))+z*math.cos(math.radians(angle)))/.07
            ny=math.sqrt(1-radial*radial)
            nx=-math.sin(math.radians(angle))*radial
            rebound=math.degrees(math.atan2(2*ny*nx,2*ny*ny-1))
            expected=(((2-3+10)*.85+rebound)*sign+180)%360-180
            self.db.execute('UPDATE player_game_batting SET hla_model_version=2, actual_bat_angle_deg=?,contact_offset_z_m=?,final_hla_deg=? WHERE count_seq=?',
                            (angle,z,expected,seq))
        self.assertEqual(self.query(0)[0]['hla_mismatches'],0)
        self.assertEqual(len(self.query(1)),2)

    def test_signed_replay_clamping_legacy_and_fixed_draw_comparison(self):
        self.add(1,'MarginalContact',1,40,.001,.1)
        self.add(2,'MarginalContact',-1,40,.001,.1)
        self.add(3,'SolidContact',1,math.degrees(.03),.001,.01)
        self.add(4,'WeakContact',None,0,0,0)
        coverage=self.query(0)[0]
        self.assertEqual(list(coverage.values()),[4,3,1,0])
        rows=self.query(1)
        marginal=[r for r in rows if r['adjusted_contact_type']=='MarginalContact']
        self.assertEqual(len(marginal),2)
        self.assertTrue(all(r['rebound_clamped_contacts']==1 for r in marginal))
        self.assertTrue(all(r['launch_foul_directions']==1 and r['original_timing_foul_directions']==0 for r in marginal))
        dist=self.query(2)
        self.assertEqual(sum(r['contacts'] for r in dist if r['variant']=='current'),3)
        transition=self.query(3)
        self.assertEqual(transition[0]['contacts'],2)
        self.assertEqual(transition[0]['current_direction'],'foul_direction')
        self.assertEqual(transition[0]['original_timing_direction'],'fair_direction')
        self.db.execute('update player_game_batting set final_hla_deg=0 where count_seq=1')
        self.assertEqual(self.query(0)[0]['hla_mismatches'],1)


if __name__=='__main__':
    unittest.main()
