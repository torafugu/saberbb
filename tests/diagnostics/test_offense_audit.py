import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest

import offense_audit


class OffenseAuditTest(unittest.TestCase):
    def fixture(self, path):
        with sqlite3.connect(path) as conn:
            conn.executescript('''
                CREATE TABLE game(id INTEGER PRIMARY KEY,season INTEGER,actual_date TEXT);
                INSERT INTO game VALUES(1,2026,'2026-04-01'),(2,2027,'2027-04-01');
                CREATE TABLE player_game_batting(game_id,count_seq,batter_id,result,launch_speed,launch_angle,
                    adjusted_contact_type,length_offset_m,thickness_offset_m,first_bounce_time,final_hla_deg,polar_distance);
                INSERT INTO player_game_batting VALUES
                    (1,1,7,'Take',0,0,'Take',0,0,NULL,0,0),
                    (1,2,7,'Foul',25,35,'WeakContact',.1,.04,NULL,60,20),
                    (1,3,7,'HomeRun',45,25,'SolidContact',.01,.01,NULL,10,110),
                    (1,4,7,'Walk',0,0,'Take',0,0,NULL,0,0),
                    (1,5,7,'Strikeout',0,0,'SwungAndMiss',0,0,NULL,0,0),
                    (2,1,7,'HomeRun',47,27,'SolidContact',.01,.01,NULL,10,115);
                CREATE TABLE batter_info(player_id,batter_type);
                INSERT INTO batter_info VALUES(7,'AggressiveFreeSwinger');
                CREATE TABLE stat_cache_state(id,version,ready);
                INSERT INTO stat_cache_state VALUES(1,3,1);
                CREATE TABLE game_batting_stat_cache(game_id,player_id,ab,homerun);
                INSERT INTO game_batting_stat_cache VALUES(1,7,5,1),(2,7,1,1);
                CREATE TABLE batting_stat_totals(player_id,ab,homerun);
                INSERT INTO batting_stat_totals VALUES(7,6,2);
            ''')

    def test_pitches_pa_ab_hr_and_season_scope_are_distinct_without_writes(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'test.db'
            self.fixture(path)
            before = path.read_bytes()
            all_years = offense_audit.audit(path)
            selected = offense_audit.audit(path, 2026)
            leader = selected['top_batters'][0]
            self.assertEqual((leader['pitches'], leader['pa'], leader['ab'], leader['hr']), (5, 3, 2, 1))
            self.assertEqual(all_years['top_batters'][0]['hr'], 2)
            self.assertEqual(selected['cache_mismatches'], [dict(batter_id=7, raw_ab=2, raw_hr=1, cached_ab=5, cached_hr=1)])
            self.assertEqual(selected['career_totals_mismatches'], [])
            self.assertEqual(selected['by_batter_type'][0]['hr'], 1)
            self.assertEqual(selected['home_run_geometry'][0]['hr_with_prior_bounce'], 0)
            self.assertEqual(path.read_bytes(), before)
            self.assertEqual(offense_audit.audit(path, 2099)['top_batters'], [])
            json.dumps(all_years, allow_nan=False)

    def test_missing_database_is_not_created(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'missing.db'
            with self.assertRaises(sqlite3.OperationalError):
                offense_audit.audit(path)
            self.assertFalse(path.exists())

    def test_cli_requires_database_environment(self):
        env = dict(os.environ)
        env.pop('SABERBB_DB', None)
        result = subprocess.run([sys.executable, offense_audit.__file__], env=env, text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('SABERBB_DB is not set', result.stderr)
