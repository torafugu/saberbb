import csv
import hashlib
import importlib.util
import json
import math
import re
import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/diagnostics'))
import contact_state_probe as probe


def state(delay=.003):
    ball=[.02,0,.9];aim=[.003,0,.001]
    bat_velocity=[0,35*math.cos(math.radians(10)),35*math.sin(math.radians(10))]
    velocity=[1,-38,-2]
    return dict(input=dict(reference_time_sec_since_release=.4,ball_position_world_m=ball,
        ball_velocity_world_ms=velocity,aim_error_world_m=aim,timing_offset_sec=delay,
        bat_speed_ms=35,bat_angle_deg=30,attack_angle_deg=10,handedness_sign=1,
        legacy_contact_offset_xz_m=[.1,.001],legacy_timing_impact_x_m=.097,
        legacy_timing_position_scale=.75),
        bat_position_world_m=[b-a-v*delay for b,a,v in zip(ball,aim,bat_velocity)],
        bat_ideal_position_world_m=[b-a for b,a in zip(ball,aim)],
        bat_axis_world=[math.cos(math.radians(30)),0,.5],bat_velocity_world_ms=bat_velocity,
        bat_angular_velocity_world_rad_s=[0,0,0],
        relative_position_world_m=[a+v*delay for a,v in zip(aim,bat_velocity)],
        relative_velocity_world_ms=[a-b for a,b in zip(velocity,bat_velocity)])


class ContactStateTest(unittest.TestCase):
    def test_identity_reconstruction_and_corrupt_payload(self):
        for delay in (-.003,0,.003):
            relative,checks=probe.validate_state(state(delay))
            self.assertAlmostEqual(relative[0],.003)
            self.assertLess(max(checks.values()),1e-12)
        corrupted=state();corrupted['relative_position_world_m'][0]+=.1
        with self.assertRaises(ValueError): probe.validate_state(corrupted)
        corrupted=state();corrupted['input']['bat_speed_ms']=30
        with self.assertRaises(ValueError): probe.validate_state(corrupted)

    def test_phase_has_no_legacy_x_addition(self):
        snapshot=state()
        before,_=probe.validate_state(snapshot)
        snapshot['input']['legacy_timing_impact_x_m']=100
        after,_=probe.validate_state(snapshot)
        self.assertEqual(before,after)
        self.assertGreater(after[1],0)

    def test_database_migration_coverage_and_read_only_replay(self):
        with tempfile.TemporaryDirectory() as folder:
            folder=Path(folder);path=folder/'states.db'
            db=sqlite3.connect(path)
            ddl=(ROOT/'migrations/ddl/player_game_batting_table.sql').read_text()
            db.executescript(re.sub(r'^DROP TABLE .*?;\s*','',ddl))
            migration=(ROOT/'migrations/upgrades/add_contact_state_shadow.sql').read_text()
            db.executescript(migration)
            for seq,kind in [(1,'SolidContact'),(2,'SwungAndMiss'),(3,'WeakContact'),
                             (4,'MarginalContact'),(5,'SolidContact'),(6,'SolidContact'),(7,'Take')]:
                values={r[1]:(0 if 'REAL' in r[2] or 'INT' in r[2] else '')
                        for r in db.execute('pragma table_info(player_game_batting)') if r[3]}
                values.update(game_id=10,count_seq=seq,adjusted_contact_type=kind,result='Out',launch_speed=46,launch_angle=10)
                db.execute('insert into player_game_batting ('+','.join(values)+') values ('
                           +','.join('?' for _ in values)+')',list(values.values()))
            for seq in (1,2,4,5,6,8):
                payload=state();reason=None;source=probe.SOURCE
                if seq==4: reason='invalid_pitch_flight_time'
                if seq==5: source='future'
                if seq==6: payload['relative_position_world_m'][0]=9
                db.execute('insert into player_game_contact_state values (10,?,1,?,?,?)',
                           (seq,source,reason,None if reason else json.dumps(payload)))
            db.executescript(migration) # Idempotent upgrade preserves earlier state rows.
            self.assertEqual(db.execute('select count(*) from player_game_contact_state').fetchone()[0],6)
            with self.assertRaises(sqlite3.IntegrityError):
                db.execute("insert into player_game_contact_state values (10,99,1,'test',NULL,NULL)")
            db.commit();db.close()
            before=hashlib.sha256(path.read_bytes()).hexdigest()
            output=folder/'reports'
            coverage=probe.run(path,10,10,output)
            self.assertEqual(coverage,dict(first_game_id=10,last_game_id=10,swings=7,
                                          recorded_swings=6,unrecorded_batting_swings=1,valid_states=3,cylinder_intersections=3))
            def read(name):
                with (output/name).open() as f:return list(csv.DictReader(f))
            self.assertEqual(sum(int(r['swings']) for r in read('state_status.csv')),7)
            summaries=read('state_collision_summary.csv')
            self.assertEqual(len(summaries),19)
            self.assertFalse(any(r['variant']=='current' and r['adjusted_contact_type']=='SwungAndMiss' for r in summaries))
            self.assertEqual(sum(int(r['contacts']) for r in read('state_collision_distribution.csv')),19)
            self.assertEqual(len(read('state_contacts.csv')),3)
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(),before)
            with self.assertRaises(FileExistsError):probe.run(path,10,10,output)
            empty=probe.run(path,20,30,folder/'empty')
            self.assertEqual(empty['swings'],0)


if __name__=='__main__':unittest.main()
