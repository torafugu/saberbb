import sqlite3
import unittest

from wild_pitch_advancement_audit import replay_game, wild_pitch_advancement


class WildPitchAdvancementTest(unittest.TestCase):
    def test_ball_four_distinguishes_forced_and_additional_advancement(self):
        full = wild_pitch_advancement((10, 20, 30), 40, {"ball_four": True}, 1)
        self.assertEqual(full["advancement"], "forced_by_walk_only")
        self.assertEqual(full["runner_ids_advanced_by_wild_pitch"], [])
        self.assertEqual(full["runners_after"], (40, 10, 20))

        gaps = wild_pitch_advancement((10, None, 30), 40, {"ball_four": True}, 1)
        self.assertEqual(gaps["advancement"], "extra_advance_beyond_walk")
        self.assertEqual(gaps["runner_ids_advanced_by_wild_pitch"], [30])

        second_only = wild_pitch_advancement((None, 20, None), 40, {"ball_four": True}, 0)
        self.assertEqual(second_only["runner_ids_advanced_by_wild_pitch"], [20])

    def test_replay_follows_steal_snapshot_and_resets_each_half(self):
        with sqlite3.connect(":memory:") as conn:
            conn.executescript("""
                CREATE TABLE count(game_id, seq, inning_seq, inning_tb, point);
                CREATE TABLE player_game_batting(game_id, count_seq, batter_id, result);
                CREATE TABLE player_game_pitching(game_id, count_seq, actual_location_x, actual_location_y);
                CREATE TABLE player_game_running(game_id, count_seq, seq, event,
                    ruling, runner_1st_id, runner_2nd_id, runner_3rd_id);
                INSERT INTO count VALUES
                    (1, 1, 1, 'Top', 0), (1, 2, 1, 'Top', 0), (1, 3, 1, 'Top', 0),
                    (1, 4, 1, 'Bottom', 0), (1, 6, 1, 'Bottom', 0);
                INSERT INTO player_game_batting VALUES
                    (1, 1, 10, 'Walk'), (1, 3, 20, 'Walk'), (1, 4, 30, 'WildPitch'),
                    (1, 6, 30, 'WildPitch');
                INSERT INTO player_game_pitching VALUES
                    (1, 1, 0.1, 0.2), (1, 2, 0.5, 0.5), (1, 3, 0.0, -2.3);
                INSERT INTO player_game_running VALUES (1, 2, 1, 'BaseSteal', 'Out', 10, NULL, NULL);
            """)
            events = {3: {"ball_four": True}, 4: {"ball_four": False}, 6: {"ball_four": False}}
            replay_game(conn, 1, events)
        self.assertEqual(events[3]["advancement"], "forced_by_walk_only")
        self.assertEqual(events[3]["runners_before"], (10, None, None))
        self.assertEqual(events[4]["advancement"], "bases_empty")
        self.assertEqual(events[6]["advancement"], "unknown")

    def test_inconsistent_scoring_does_not_become_no_advancement(self):
        result = wild_pitch_advancement((None, None, 30), 40, {"ball_four": False}, 0)
        self.assertEqual(result["advancement"], "unknown")


if __name__ == "__main__":
    unittest.main()
