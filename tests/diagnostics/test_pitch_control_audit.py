import sqlite3
import unittest

from pitch_control_audit import report


class PitchControlAuditTest(unittest.TestCase):
    def test_saved_edge_outcomes_use_the_pre_pitch_count(self):
        with sqlite3.connect(":memory:") as conn:
            conn.executescript("""
                CREATE TABLE game(id, home_team_id, away_team_id, season, game_type, actual_date);
                CREATE TABLE player_info(id, team_id, first_name, last_name);
                CREATE TABLE pitcher_info(player_id, control);
                CREATE TABLE game_pitching_stat_cache(
                    game_id, player_id, outs, earned_runs, bb, so, hits, home_runs
                );
                CREATE TABLE player_game_pitching(
                    game_id, count_seq, pitcher_id, aim_zone, aim_location_x, aim_location_y,
                    actual_location_x, actual_location_y, pitch_type, speed
                );
                CREATE TABLE count(game_id, inning_seq, inning_tb, seq, ball, strike);
                CREATE TABLE player_game_batting(game_id, count_seq, result, decision_swung);
                INSERT INTO game VALUES (10, 1, 2, 2026, 'Regular', '2026-05-01');
                INSERT INTO player_info VALUES (42, 1, 'Fixture', 'Pitcher');
                INSERT INTO pitcher_info VALUES (42, 2.0);
                INSERT INTO game_pitching_stat_cache VALUES (10, 42, 3, 0, 0, 1, 0, 0);
                INSERT INTO count VALUES
                    (10, 1, 'Top', 1, 0, 1), (10, 1, 'Top', 2, 0, 2),
                    (10, 1, 'Top', 3, 1, 2), (10, 1, 'Top', 4, 1, 2),
                    (10, 1, 'Top', 5, 2, 2), (10, 1, 'Top', 6, 0, 0),
                    (10, 1, 'Bottom', 7, 0, 1);
                INSERT INTO player_game_pitching
                    SELECT game_id, seq, 42, 'LowInside',
                           CASE WHEN seq = 3 THEN -1.2 WHEN seq = 7 THEN -.9 ELSE -.75 END,
                           CASE WHEN seq = 3 THEN -1.2 WHEN seq = 7 THEN -.9 ELSE -.75 END,
                           CASE WHEN seq IN (3, 5) THEN -1.1 ELSE -.8 END,
                           -.7, 'Cutter', 40 FROM count;
                INSERT INTO player_game_batting VALUES
                    (10, 1, 'Take', 0), (10, 2, 'Take', 0), (10, 3, 'Take', 0),
                    (10, 4, 'Foul', 1), (10, 5, 'Take', 0), (10, 6, 'Strikeout', NULL);
            """)
            prior = report(conn, 2026)
            audit = report(conn, 2026, include_count_margins=True)

        player = audit["pitchers"][0]["by_count_and_margin"]
        self.assertEqual(player, audit["league"]["by_count_and_margin"])
        self.assertEqual(player["1-2"]["Edge"], {
            "pitches": 2, "actual_outside": 1, "actual_outside_rate": 0.5,
            "ball_count_advances": 1, "ball_count_advance_rate": 0.5,
            "in_zone_outcomes": {"foul": 1},
            "out_of_zone_outcomes": {"called_ball": 1},
        })
        self.assertEqual(player["0-2"]["Out"]["ball_count_advances"], 1)
        self.assertEqual(player["2-2"]["Edge"]["in_zone_outcomes"], {"strikeout_unknown_swing": 1})
        self.assertEqual(player["0-0"]["Other"]["in_zone_outcomes"], {"unrecorded_or_other": 1})
        audit["pitchers"][0].pop("by_count_and_margin")
        audit["league"].pop("by_count_and_margin")
        self.assertEqual(audit, prior)


if __name__ == "__main__":
    unittest.main()
