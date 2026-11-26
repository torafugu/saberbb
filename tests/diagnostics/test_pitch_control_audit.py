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
                INSERT INTO player_info VALUES (43, 1, 'Relief', 'Pitcher');
                INSERT INTO pitcher_info VALUES (43, 1.0);
                INSERT INTO game_pitching_stat_cache VALUES (10, 43, 1, 0, 1, 0, 0, 0);
                INSERT INTO count VALUES
                    (10, 2, 'Top', 8, 1, 0), (10, 2, 'Top', 9, 2, 0),
                    (10, 2, 'Top', 10, 3, 0), (10, 2, 'Top', 11, 3, 1),
                    (10, 2, 'Top', 12, 3, 2), (10, 2, 'Top', 13, 3, 2),
                    (10, 2, 'Top', 14, 0, 0);
                INSERT INTO player_game_pitching
                    SELECT game_id, seq, CASE WHEN seq = 14 THEN 43 ELSE 42 END,
                           'LowInside', -(1.0-1.0/3.0), -(1.0-1.0/3.0),
                           CASE seq WHEN 14 THEN -1.2 WHEN 13 THEN -1.1
                                    WHEN 12 THEN -1.05 ELSE -.8 END,
                           -.7, 'Cutter', 40 FROM count WHERE seq >= 8;
                INSERT INTO player_game_batting VALUES
                    (10, 8, 'Take', 0), (10, 9, 'Take', 0), (10, 10, 'Take', 0),
                    (10, 11, 'Take', 0), (10, 13, 'Foul', 1), (10, 14, 'Walk', 0);
            """)
            prior = report(conn, 2026)
            audit = report(conn, 2026, include_count_margins=True)
            location_audit = report(conn, 2026, include_three_ball_locations=True)
            endings_audit = report(conn, 2026, include_count_endings=True)

        player = audit["pitchers"][0]["by_count_and_margin"]
        self.assertEqual(player["1-2"], audit["league"]["by_count_and_margin"]["1-2"])
        self.assertEqual(player["1-2"]["Edge"], {
            "pitches": 2, "actual_outside": 1, "actual_outside_rate": 0.5,
            "ball_count_advances": 1, "ball_count_advance_rate": 0.5,
            "in_zone_outcomes": {"foul": 1},
            "out_of_zone_outcomes": {"called_ball": 1},
        })
        self.assertEqual(player["0-2"]["Out"]["ball_count_advances"], 1)
        self.assertEqual(player["2-2"]["Edge"]["in_zone_outcomes"], {"strikeout_unknown_swing": 1})
        self.assertEqual(player["0-0"]["Other"]["in_zone_outcomes"], {"unrecorded_or_other": 1})
        for pitcher in audit["pitchers"]:
            pitcher.pop("by_count_and_margin")
        audit["league"].pop("by_count_and_margin")
        self.assertEqual(audit, prior)

        league_locations = location_audit["league"].pop("three_ball_locations")
        pitcher_locations = {
            p["player_id"]: p.pop("three_ball_locations") for p in location_audit["pitchers"]
        }
        self.assertEqual(location_audit, prior)
        self.assertEqual([row["count"] for row in league_locations], ["3-0", "3-1", "3-2"])
        self.assertEqual([row["pitches"] for row in league_locations], [1, 1, 2])
        self.assertEqual(league_locations[1]["outside_unknown_decisions"], 1)
        full_count = league_locations[2]
        self.assertEqual(full_count["zone"], "LowInside")
        self.assertEqual(full_count["margin"], "Wide")
        self.assertAlmostEqual(full_count["aim_x"], -2 / 3)
        self.assertAlmostEqual(full_count["aim_y"], -2 / 3)
        self.assertAlmostEqual(full_count["aim_depth"], 1 / 3)
        self.assertEqual(full_count["actual_outside_rate"], 1.0)
        self.assertEqual(full_count["outside_swings"], 1)
        self.assertEqual(full_count["outside_takes"], 1)
        self.assertEqual(full_count["walks"], 1)
        self.assertEqual(pitcher_locations[42][-1]["walks"], 0)
        self.assertEqual(pitcher_locations[43][0]["walks"], 1)
        self.assertAlmostEqual(full_count["mean_error_x"], -1.15 + 2 / 3)
        self.assertAlmostEqual(full_count["rms_error_x"], (
            ((-1.1 + 2 / 3) ** 2 + (-1.2 + 2 / 3) ** 2) / 2
        ) ** 0.5)

        league_endings = endings_audit["league"].pop("by_count_endings")
        for pitcher in endings_audit["pitchers"]:
            endings = pitcher.pop("by_count_endings")
            self.assertEqual(
                sum(row["finished_appearances"] for row in endings.values()),
                pitcher["batters_faced"],
            )
        self.assertEqual(endings_audit, prior)
        self.assertEqual(league_endings["1-2"]["pitches"], 2)
        self.assertEqual(league_endings["1-2"]["ball_count_advances_without_walk"], 1)
        self.assertEqual(league_endings["1-2"]["finished_appearances"], 0)
        self.assertEqual(league_endings["2-2"]["strikeouts_unknown_swing"], 1)
        self.assertEqual(league_endings["2-2"]["finished_appearances"], 1)
        self.assertEqual(league_endings["3-2"]["pitches"], 2)
        self.assertEqual(league_endings["3-2"]["walks"], 1)
        self.assertEqual(league_endings["3-2"]["ball_count_advances_without_walk"], 0)
        self.assertEqual(
            sum(row["finished_appearances"] for row in league_endings.values()),
            prior["league"]["bf"],
        )


if __name__ == "__main__":
    unittest.main()
