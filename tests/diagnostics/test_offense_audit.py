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

    def test_extra_base_uses_fair_results_and_fielding_event_order(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'test.db'
            self.fixture(path)
            with sqlite3.connect(path) as conn:
                conn.executescript("""
                    ALTER TABLE player_game_batting ADD outbound_result DEFAULT 'InField';
                    ALTER TABLE player_game_batting ADD fence_impact_time;
                    ALTER TABLE player_game_batting ADD first_bounce_distance;
                    ALTER TABLE player_game_batting ADD first_bounce_angle;
                    ALTER TABLE player_game_batting ADD fielder_position;
                    INSERT INTO player_game_batting VALUES
                        (1,6,7,'Single',42,25,'SolidContact',0,0,4,0,90,'InField',3,95,0,'CF'),
                        (1,7,7,'Double',40,20,'SolidContact',0,0,2,13,90,'InField',5,85,13,'CF'),
                        (1,8,7,'Out',40,20,'SolidContact',0,0,4,13,90,'InField',5,85,13,'CF'),
                        (1,9,7,'Out',40,20,'SolidContact',0,0,NULL,60,90,'Foul',2,85,13,'CF'),
                        (1,10,7,'Double',40,20,'SolidContact',0,0,2,13,90,'GroundRuleDouble',NULL,85,13,NULL);
                    INSERT INTO player_game_batting VALUES
                        (1,11,7,'Triple',41,19,'SolidContact',0,0,2,25,90,'InField',NULL,85,25,'CF'),
                        (2,11,7,'Triple',41,19,'SolidContact',0,0,2,25,90,'InField',NULL,85,25,'CF');
                    ALTER TABLE game_batting_stat_cache ADD triple INTEGER DEFAULT 0;
                    UPDATE game_batting_stat_cache SET triple=1 WHERE game_id=2;
                    ALTER TABLE batting_stat_totals ADD triple INTEGER DEFAULT 0;
                    CREATE TABLE player_game_fielding(game_id,count_seq,seq,time_to_field,catch_type,play_type);
                    INSERT INTO player_game_fielding VALUES
                        (1,6,1,5,'BounceCatch','TouchPlay'),
                        (1,6,2,0,NULL,'ForcePlay'),
                        (1,7,1,4,'BounceCatch','TouchPlay'),
                        (1,8,1,3,'AirCatch','CatchPlay');
                    CREATE TABLE player_game_running(game_id,count_seq,seq,event,defense_time,
                        target_runner_id,runner_time,throw_target_base);
                    INSERT INTO player_game_running VALUES
                        (1,6,1,'BaseSteal',2,9,2,'Second'),
                        (1,6,2,'GrounderPlay',7,9,6,'Home'),
                        (1,6,3,'DoublePlay',8,7,8,'Second'),
                        (1,7,1,'GrounderPlay',8,7,7,'Second');
                """)
            before = path.read_bytes()
            audited = offense_audit.audit(path, 2026, extra_base=True)
            self.assertEqual([(r['game_id'], r['count_seq'], r['batter_id']) for r in audited['triples']], [(1, 11, 7)])
            self.assertEqual(audited['triple_cache_checks']['game_batting_stat_cache'],
                             [dict(game_id=1, player_id=7, raw_triples=1, cached_triples=0)])
            self.assertEqual(audited['triple_cache_checks']['batting_stat_totals'],
                             [dict(player_id=7, raw_triples=2, cached_triples=0)])
            report = audited['extra_base']
            self.assertEqual(sum(row['fair_contacts'] for row in report['ev_vla']), 6)
            self.assertEqual(report['power_window'], dict(fair_contacts=5, home_runs=1, airborne_wall_impacts=1))
            groups = {row['location']: row['outcomes'] for row in report['conversion']}
            self.assertEqual(groups['wall_impact']['Single'], 1)
            self.assertEqual(groups['deep_gap_first_bounce']['Double'], 1)
            self.assertEqual(next(row['outcomes']['Out'] for row in report['conversion'] if row['location'] == 'other_outfield' and row['fielding'] == 'AirCatch'), 1)
            self.assertEqual(groups['ground_rule_double']['Double'], 1)
            batter_times = [row for row in report['timing_seconds'] if row['metric'] == 'batter_target_time']
            self.assertEqual([(row['result'], row['target_base'], row['mean']) for row in batter_times],
                             [('Double', 'Second', 7)])
            self.assertEqual(path.read_bytes(), before)

    def test_missing_database_is_not_created(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'missing.db'
            with self.assertRaises(sqlite3.OperationalError):
                offense_audit.audit(path)
            self.assertFalse(path.exists())

    def test_league_batting_compares_qualified_leaders_with_all_regular_season_batters(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'test.db'
            with sqlite3.connect(path) as conn:
                conn.executescript('''
                    CREATE TABLE game(id INTEGER PRIMARY KEY, season INTEGER, game_type TEXT,
                        actual_date TEXT, away_team_id INTEGER, home_team_id INTEGER);
                    INSERT INTO game VALUES
                        (1,2026,'Regular','2026-04-01',1,2),
                        (2,2026,'Regular','2026-04-02',1,3),
                        (3,2026,'Regular','1900-01-01',1,2),
                        (4,2027,'Playoff','2027-04-01',1,2),
                        (5,2027,'Regular',NULL,1,2);
                    CREATE TABLE player_info(id PRIMARY KEY,team_id,first_name,last_name);
                    INSERT INTO player_info VALUES (10,1,'A','A'),(20,2,'B','B'),(30,1,'C','C'),(40,3,'D','D');
                    CREATE TABLE player_game_batting(game_id,count_seq,batter_id,result,
                        launch_speed,launch_angle,outbound_result,first_bounce_time,fence_impact_time);
                    INSERT INTO player_game_batting VALUES
                        (1,1,10,'Single',32,9.9,'InField',1,NULL),
                        (1,2,10,'Strikeout',0,0,'InField',NULL,NULL),
                        (1,3,10,'Out',41,25,'InField',NULL,3),
                        (1,4,10,'Walk',0,0,'InField',NULL,NULL),
                        (2,1,10,'HomeRun',50,25,'HomeRun',NULL,NULL),
                        (2,2,10,'Double',43,20,'InField',4,3),
                        (1,5,20,'Strikeout',0,0,'InField',NULL,NULL),
                        (1,6,20,'Single',0,0,'InField',NULL,NULL),
                        (1,7,20,'Out',25,50,'InField',NULL,NULL),
                        (2,3,40,'Walk',0,0,'InField',NULL,NULL),
                        (2,4,40,'Walk',0,0,'InField',NULL,NULL),
                        (2,5,40,'Walk',0,0,'InField',NULL,NULL),
                        (2,6,10,'Foul',45,25,'Foul',NULL,NULL),
                        (3,1,10,'HomeRun',50,25,'HomeRun',NULL,NULL),
                        (4,1,10,'HomeRun',50,25,'HomeRun',NULL,NULL),
                        (5,1,10,'HomeRun',50,25,'HomeRun',NULL,NULL);
                    CREATE TABLE count(game_id,seq,point);
                    INSERT INTO count VALUES (1,3,1),(1,7,1);
                    CREATE TABLE player_game_running(game_id,count_seq,seq,event);
                    INSERT INTO player_game_running VALUES (1,3,1,'TagUp'),(1,7,1,'TagUp'),(1,7,2,'FlyPlay');
                    CREATE TABLE player_game_fielding(game_id,count_seq,seq,time_to_field);
                    INSERT INTO player_game_fielding VALUES (1,3,1,2),(2,2,2,1),(2,2,1,5);
                ''')
                conn.executemany('INSERT INTO player_game_batting VALUES (2,?,30,\'Single\',25,10,\'InField\',1,NULL)',
                                 ((seq,) for seq in range(7, 12)))
                conn.executescript("""
                    ALTER TABLE player_game_batting ADD pitcher_id;
                    UPDATE player_game_batting SET pitcher_id=60 WHERE game_id=1;
                    UPDATE player_game_batting SET pitcher_id=70 WHERE game_id=2;
                    UPDATE player_game_batting SET pitcher_id=71 WHERE game_id=2 AND count_seq IN (2,3);
                    UPDATE player_game_batting SET pitcher_id=999 WHERE game_id=2 AND count_seq=4;
                    INSERT INTO player_info VALUES
                        (60,2,'P','Elite'),(70,3,'P','Short'),(71,3,'P','Middle'),(72,1,'P','High');
                    CREATE TABLE game_pitching_stat_cache(game_id,player_id,outs,runs,earned_runs,unclassified_runs);
                    INSERT INTO game_pitching_stat_cache VALUES
                        (1,60,18,3,1,2),(2,70,2,2,2,0),(2,71,9,1,1,0),(1,72,3,1,1,0),(2,72,3,1,1,0),
                        (3,60,0,90,90,0),(4,60,0,90,90,0);
                """)
            before = path.read_bytes()
            env = dict(os.environ, SABERBB_DB=str(path))
            proc = subprocess.run([sys.executable, offense_audit.__file__, '--league-batting', '--top', '2'],
                                  env=env, text=True, capture_output=True, check=True)
            report = json.loads(proc.stdout)
            self.assertEqual(report['scope'], dict(season=2026, completed_regular_games=2,
                                                   games_with_terminal_batting_logs=2))
            self.assertEqual(report['qualified_batters'], 3)
            self.assertEqual(report['qualified_without_at_bats'], 1)
            self.assertEqual(report['qualified_batting_avg_counts'], {'at_least_500': 1, '400_to_under_500': 0})
            self.assertEqual(report['max_home_runs'], 1)
            first, second = report['top_qualified']
            self.assertEqual((first['player']['id'], first['minimum_pa'], first['pa'], first['ab']), (10, 6, 6, 4))
            self.assertEqual((second['player']['id'], second['minimum_pa']), (20, 3))
            self.assertEqual((first['ba'], first['strikeouts_per_ab'], first['babip']), (0.75, 0.25, 2/3))
            self.assertEqual(first['sacrifice_flies'], 1)
            self.assertEqual(second['sacrifice_flies'], 0)
            self.assertEqual(first['bip_by_launch_angle']['Grounder']['hits'], 1)
            self.assertEqual(first['bip_by_launch_angle']['Liner']['hits'], 1)
            self.assertEqual(first['bip_by_launch_angle']['Fly']['balls'], 1)
            self.assertEqual(second['bip_by_launch_angle']['PopUp']['hit_rate'], 0)
            self.assertEqual(second['bip_by_launch_angle']['Unknown']['hits'], 1)
            self.assertEqual(report['league']['pa'], 17)
            self.assertEqual(report['league']['babip'], 0.8)
            self.assertEqual(report['top_qualified_pool']['babip'], 0.6)
            power = report['league_power']
            self.assertEqual((power['fair_contacts'], power['home_runs'], power['trajectory_home_runs'],
                              power['airborne_wall_impacts']), (3, 1, 1, 1))
            self.assertEqual(power['home_runs_per_window_contact'], 1/3)
            self.assertEqual(sum(cell['fair_contacts'] for cell in report['league_ev_vla']),
                             power['measured_fair_contacts'])
            self.assertEqual(path.read_bytes(), before)

            matchup = offense_audit.audit(path, league_batting=True, pitcher_matchups=True)['pitcher_matchups']
            self.assertTrue(matchup['pitching_cache_covers_all_completed_games'])
            pitching = matchup['pitching_by_cohort']
            self.assertEqual(pitching['qualified']['outs'], 33)
            self.assertEqual(pitching['unqualified']['outs'], 2)
            self.assertEqual(pitching['league']['outs'], 35)
            self.assertEqual(pitching['qualified_era_under_2']['era'], 1.5)
            self.assertEqual(pitching['qualified_era_under_2']['ra9'], 4.5)
            self.assertEqual(pitching['qualified_era_2_to_under_4']['outs'], 9)
            self.assertEqual(pitching['qualified_era_at_least_4']['outs'], 6)
            batter = matchup['qualified_batter_matchups'][0]['by_pitcher_cohort']
            self.assertEqual((batter['qualified_era_under_2']['pa'], batter['qualified_era_under_2']['ab']), (4, 2))
            self.assertEqual(batter['qualified_era_under_2']['walks'], 1)
            self.assertEqual(batter['unqualified']['hr'], 1)
            self.assertEqual(batter['qualified_era_2_to_under_4']['doubles'], 1)
            self.assertEqual(sum(row['pa'] for row in matchup['league_batting_by_pitcher_cohort'].values()), 17)
            self.assertEqual(matchup['league_batting_by_pitcher_cohort']['unknown']['pa'], 1)
            self.assertEqual(sum(matchup['qualified_batting_avg_distribution'].values()), 2)
            self.assertEqual(path.read_bytes(), before)

    def test_babip_contact_mix_uses_shared_bands_and_excludes_target_from_reference(self):
        from collections import Counter
        first, second, sparse = (4, 4), (4, 5), (5, 5)
        target = Counter({(first, True): 15, (first, False): 5,
                          (second, True): 3, (second, False): 7, (sparse, True): 1})
        other = Counter({(first, True): 10, (first, False): 10,
                         (second, True): 12, (second, False): 28, (sparse, True): 2})
        line = dict(player={'id': 1}, pa=40, ba=.500, babip=.600, balls_in_play=32)
        row = offense_audit.babip_contact_mix_report([line], {1: target, 2: other})['qualified_batters'][0]
        self.assertEqual((row['measured_balls_in_play'], row['compared_balls_in_play']), (31, 30))
        self.assertEqual(row['compared_observed_babip'], 18 / 30)
        self.assertEqual(row['expected_babip_at_target_mix'], 13 / 30)
        self.assertEqual(row['other_babip_on_shared_bands'], 22 / 60)
        self.assertAlmostEqual(row['contact_mix_difference'] + row['within_band_difference'],
                               18 / 30 - 22 / 60)
        self.assertFalse(row['bands'][-1]['used_in_comparison'])
        unmatched = offense_audit.babip_contact_mix_report([line], {1: target})['qualified_batters'][0]
        self.assertIsNone(unmatched['within_band_difference'])
        json.dumps(unmatched, allow_nan=False)

    def test_hard_contact_uses_shared_geometry_and_observed_first_base_clocks(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'test.db'
            with sqlite3.connect(path) as conn:
                conn.executescript('''
                    CREATE TABLE game(id INTEGER PRIMARY KEY, season INTEGER, game_type TEXT,
                        actual_date TEXT, away_team_id INTEGER, home_team_id INTEGER);
                    INSERT INTO game VALUES (1,2026,'Regular','2026-04-01',1,2),(2,2026,'Playoff','2026-09-01',1,2);
                    CREATE TABLE player_info(id PRIMARY KEY,team_id,first_name,last_name);
                    INSERT INTO player_info VALUES (7,1,'A','A'),(8,2,'B','B');
                    CREATE TABLE player_game_batting(game_id,count_seq,batter_id,result,launch_speed,launch_angle,
                        outbound_result,first_bounce_time,fence_impact_time,final_hla_deg,first_bounce_distance,
                        first_bounce_angle,contact_offset_z_m,adapted_vertical_offset_m);
                    INSERT INTO player_game_batting VALUES
                        (1,1,7,'Single',45,0,'InField',.8,NULL,-5,15,-4,-.005,-.003),
                        (1,2,7,'Single',46,1,'InField',1.4,NULL,-5,17,-4,.003,.004),
                        (1,3,7,'Single',45,3,'InField',.5,NULL,5,18,4,NULL,.01),
                        (1,4,7,'Out',49,7,'InField',.5,NULL,NULL,16,9,.008,NULL),
                        (1,5,7,'HomeRun',51,30,'HomeRun',NULL,NULL,5,NULL,NULL,.032,.030),
                        (1,6,8,'Out',45,0,'InField',.6,NULL,-5,15,-5,-.002,-.003),
                        (1,7,8,'Single',46,2,'InField',.6,NULL,-5,15,-5,.003,.001),
                        (1,8,8,'Out',45,4,'InField',.4,NULL,5,15,5,-.004,-.003),
                        (1,9,8,'Out',45,9,'InField',.8,NULL,-5,19,-4,-.004,-.002),
                        (1,10,8,'Out',44.99,0,'InField',.6,NULL,-5,15,-5,0,0),
                        (1,11,8,'Out',47.5,10,'InField',.6,NULL,-5,15,-5,.013,.011),
                        (2,1,7,'HomeRun',51,31,'HomeRun',NULL,NULL,5,NULL,NULL,.033,.031);
                    CREATE TABLE count(game_id,seq,point);
                    CREATE TABLE player_game_running(game_id,count_seq,seq,event,throw_target_base,target_runner_id,
                        defense_time,runner_time,ruling);
                    INSERT INTO player_game_running VALUES
                        (1,1,1,'BaseSteal','Second',8,4,3,'Safe'),
                        (1,1,2,'GrounderPlay','First',7,3.4,3,'Safe'),
                        (1,2,1,'GrounderPlay','First',7,3,3,'Safe'),
                        (1,2,2,'DoublePlay','Second',8,4,3,'Safe'),
                        (1,3,1,'GrounderPlay','Second',8,4,3,'Safe'),
                        (1,6,1,'GrounderPlay','First',8,2,3,'Out'),
                        (1,7,1,'GrounderPlay','First',8,2,3,'Safe'),
                        (1,8,1,'GrounderPlay','First',8,0,0,'Safe'),
                        (1,9,1,'GrounderPlay','First',7,2,3,'Out');
                    CREATE TABLE player_game_fielding(game_id,count_seq,seq,time_to_field,
                        catch_fielder_position,interception_fielder_position,selection_path,catch_type);
                    INSERT INTO player_game_fielding VALUES
                        (1,1,1,3.1,'FB','SS','final_pickup','FinalPickup'),
                        (1,2,1,1,'FB','SB','interception','BounceCatch'),
                        (1,2,2,10,'SS',NULL,NULL,NULL),
                        (1,3,1,3.8,'CF',NULL,'final_pickup','FinalPickup'),
                        (1,6,1,1,'FB','SB','interception','BounceCatch'),
                        (1,7,1,1,'FB','SS','interception','BounceCatch');
                ''')
            before = path.read_bytes()
            env = dict(os.environ, SABERBB_DB=str(path))
            proc = subprocess.run(
                [sys.executable, offense_audit.__file__, '--league-batting', '--hard-contact', '--top', '1'],
                env=env, text=True, capture_output=True, check=True,
            )
            report = json.loads(proc.stdout)['hard_contact']
            shared = report['low_angle_shared_geometry']
            self.assertEqual(shared, dict(cells=2,
                top_qualified=dict(shared_balls=3, total_balls=4, standardized_single_rate=1.0),
                other_batters=dict(shared_balls=3, total_balls=4, standardized_single_rate=1/3)))
            stages = {row['first_base_phase']: row for row in report['low_angle_by_fielding_and_first_base']}
            late = stages['fielding_completed_at_or_after_batter']
            self.assertEqual(late['fielder_position'], 'SS')
            self.assertAlmostEqual(late['timing_seconds']['defense_minus_batter_sec']['mean'], 0.4)
            tie = stages['delivery_completed_at_or_after_batter']
            self.assertEqual(tie['timing_seconds']['defense_minus_batter_sec']['mean'], 0)
            self.assertEqual(tie['timing_seconds']['defense_after_field_sec']['mean'], 2.0)
            for phase in ('other_base', 'no_primary_grounder_play', 'first_base_times_missing',
                          'first_base_times_inconsistent', 'first_base_target_unverified'):
                self.assertEqual(stages[phase]['timing_seconds']['defense_minus_batter_sec']['n'], 0)
            self.assertEqual(stages['defense_before_batter']['timing_seconds']['defense_minus_batter_sec']['mean'], -1)
            bounces = {row['phase']: row for row in report['low_angle_by_first_bounce']}
            self.assertIsNone(bounces['projected_after_fielding']['distance_m'])
            self.assertEqual(bounces['field_time_unrecorded']['distance_m'], [10, 20])
            vertical = [row for row in report['hard_by_vertical_offset'] if row['cohort'] == 'league']
            self.assertEqual(sum(row['balls'] for row in vertical), 10)
            self.assertEqual(sum(row['balls'] for row in vertical if row['contact_offset_z_cm'] is None), 1)
            lifted = next(row for row in vertical if row['home_runs'])
            self.assertEqual((lifted['ev_ms'], lifted['contact_offset_z_cm'], lifted['vla_at_least_30']),
                             ([50, None], [3, 4], 1))
            launch = next(row for row in report['hard_by_launch_angle']
                          if row['cohort'] == 'league' and row['vla_deg'] == [30, 40])
            self.assertAlmostEqual(launch['vertical_cm']['adapted_vertical_offset']['mean'], 3.0)
            self.assertEqual(launch['vertical_cm']['swing_error_z']['n'], 0)
            self.assertEqual(path.read_bytes(), before)

    def test_targeted_comparison_uses_display_line_but_checks_raw_plays(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'test.db'
            self.fixture(path)
            with sqlite3.connect(path) as conn:
                conn.executescript('''
                    ALTER TABLE game ADD stadium_id DEFAULT 3;
                    ALTER TABLE player_game_batting ADD outbound_result DEFAULT 'InField';
                    UPDATE player_game_batting SET outbound_result='HomeRun' WHERE result='HomeRun';
                    INSERT INTO player_game_batting VALUES
                        (1,6,8,'Single',36,15,'SolidContact',0,0,4,0,70,'InField'),
                        (1,7,9,'HomeRun',51,35,'SolidContact',0,0,NULL,5,120,'HomeRun');
                    CREATE TABLE stadium(id PRIMARY KEY,name);
                    INSERT INTO stadium VALUES (3,'Park');
                    CREATE TABLE player_info(id PRIMARY KEY,team_id,first_name,last_name);
                    INSERT INTO player_info VALUES (7,1,'A','A'),(8,1,'B','B'),(9,2,'C','C');
                    ALTER TABLE batting_stat_totals ADD pa;
                    ALTER TABLE batting_stat_totals ADD single DEFAULT 0;
                    ALTER TABLE batting_stat_totals ADD double DEFAULT 0;
                    ALTER TABLE batting_stat_totals ADD triple DEFAULT 0;
                    UPDATE batting_stat_totals SET pa=525,homerun=118 WHERE player_id=7;
                ''')
            before = path.read_bytes()
            report = offense_audit.audit(path, batter_line=(525, 118))
            selected = report['selected']
            self.assertEqual(selected['player']['id'], 7)
            self.assertEqual(selected['cached']['hr'], 118)
            self.assertEqual(selected['raw']['hr'], 2)
            self.assertFalse(selected['hr_matches_cache'])
            self.assertEqual([row['player']['id'] for row in report['teammates']], [8])
            self.assertEqual(selected['batting']['contact_per_swing'], 0.75)
            self.assertEqual(selected['batting']['ev_ms']['p95'], 47)
            self.assertEqual(selected['batting']['by_stadium'][0]['hr'], 2)
            self.assertEqual(report['teammate_pool']['hr'], 0)
            scoped = offense_audit.audit(path, 2026, batter_id=7)
            self.assertEqual(scoped['selected']['raw']['hr'], 1)
            self.assertEqual(path.read_bytes(), before)
            json.dumps(report, allow_nan=False)

    def test_cli_requires_database_environment(self):
        env = dict(os.environ)
        env.pop('SABERBB_DB', None)
        result = subprocess.run([sys.executable, offense_audit.__file__], env=env, text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('SABERBB_DB is not set', result.stderr)
