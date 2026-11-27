import sqlite3
import unittest

from pitch_location_execution_audit import report


class PitchLocationBaseStateTest(unittest.TestCase):
    def test_filter_keeps_pitch_numbers_and_distinguishes_scored_wild_pitches(self):
        with sqlite3.connect(":memory:") as conn:
            conn.executescript("""
                CREATE TABLE game(id, season, game_type, actual_date, home_team_id, away_team_id);
                CREATE TABLE player_info(id, first_name, last_name);
                CREATE TABLE pitcher_info(player_id, control);
                CREATE TABLE pitch_skill(player_id, pitch_type, control);
                CREATE TABLE count(game_id, seq, inning_seq, inning_tb, point);
                CREATE TABLE player_game_batting(game_id, count_seq, batter_id, result);
                CREATE TABLE player_game_running(game_id, count_seq, seq, event,
                    runner_1st_id, runner_2nd_id, runner_3rd_id);
                CREATE TABLE player_game_pitching(game_id, count_seq, pitcher_id, pitch_type,
                    aim_zone, aim_location_x, aim_location_y, actual_location_x, actual_location_y);

                INSERT INTO game VALUES
                    (1,2026,'Regular','2026-05-01',1,2),
                    (2,2026,'Regular','2026-05-01',3,4);
                INSERT INTO player_info VALUES (427,'Subject','Pitcher'),(428,'Peer','Pitcher');
                INSERT INTO pitcher_info VALUES (427,-0.4),(428,0.0);
                WITH RECURSIVE numbers(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM numbers WHERE n<29)
                INSERT INTO count SELECT g.id,n,1,'Top',CASE WHEN n=29 THEN 1 ELSE 0 END
                    FROM game g CROSS JOIN numbers;
                INSERT INTO player_game_pitching
                    SELECT game_id,seq,426+game_id,
                        CASE WHEN seq<25 THEN 'Slider' ELSE 'FourSeamFastball' END,
                        'LowInside',-0.75,-0.75,-0.75,
                        CASE WHEN seq IN (25,27,28,29) THEN -2.5 ELSE -0.75 END FROM count;
                INSERT INTO player_game_batting
                    SELECT game_id,seq,CASE WHEN seq<27 THEN 99 ELSE 98 END,
                        CASE WHEN seq IN (25,27,28,29) THEN 'WildPitch'
                             WHEN seq=26 THEN 'Walk' ELSE 'Take' END FROM count;
            """)
            no_third = report(conn, 2026, 427, "FourSeamFastball", None, "no-third")
            runners_only = report(conn, 2026, 427, "FourSeamFastball", None, "first-or-second")
            third = report(conn, 2026, 427, "FourSeamFastball", None, "third")
            unfiltered = report(conn, 2026, 427, "FourSeamFastball", None)

        aim = no_third["by_aim_zone_and_margin"][0]
        self.assertEqual(aim["subject"]["pitches"], 4)
        self.assertEqual(aim["peers"]["usage_rate"], 1)
        self.assertEqual(aim["subject"]["wild_pitches"], 3)
        self.assertEqual(aim["subject"]["scored_wild_pitches"], 2)
        bands = runners_only["matched_by_aim_and_pitch_number"]
        self.assertEqual([row["pitch_numbers"] for row in bands], [[26, 50]])
        self.assertEqual(bands[0]["subject"]["pitches"], 2)
        # The runner starts on second at pitch 28, then scores on the next pitch.
        self.assertEqual(bands[0]["subject"]["scored_wild_pitches"], 2)
        self.assertEqual(third["by_aim_zone_and_margin"][0]["subject"]["pitches"], 1)
        self.assertEqual(unfiltered["by_aim_zone_and_margin"][0]["subject"]["pitches"], 5)
        self.assertEqual(no_third["pitches_excluded_unknown_base_state"]["subject"], 0)
