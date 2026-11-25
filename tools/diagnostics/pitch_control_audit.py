"""Report saved regular-season pitching control and walk rates as JSON."""

import argparse
import json
import math
import sqlite3
from collections import defaultdict

from diagnostic_db import db_from_env


COMPLETED = """
    SELECT id, home_team_id, away_team_id FROM game
    WHERE season = ? AND game_type = 'Regular'
      AND actual_date IS NOT NULL AND actual_date != '1900-01-01'
"""

PITCHES = """
    SELECT p.pitcher_id, p.aim_zone, p.aim_location_x, p.aim_location_y,
           p.actual_location_x, p.actual_location_y,
           c.before_balls, c.ball, b.result, b.decision_swung,
           p.pitch_type, p.speed, p.pitch_number
    FROM pitch_order AS p
    JOIN counts AS c ON c.game_id = p.game_id AND c.seq = p.count_seq
    LEFT JOIN player_game_batting AS b
        ON b.game_id = p.game_id AND b.count_seq = p.count_seq
"""


def ratio(numerator, denominator):
    return numerator / denominator if denominator else None


TERMINAL_RESULTS = {
    "Strikeout", "Walk", "HitByPitch", "Single", "Double", "Triple",
    "HomeRun", "FieldersChoice", "Out", "DoublePlay",
}


def pitch_count_summary(bands):
    summaries = []
    for band, types in sorted(bands.items()):
        pitch_types = {}
        for pitch_type, data in sorted(types.items()):
            pitch_types[pitch_type] = {
                "pitches": data["pitches"],
                "mean_speed_ms": data["speed_sum"] / data["pitches"],
                "rms_location_error": math.sqrt(data["error_sq_sum"] / data["pitches"]),
            }
        two = sum(data["two_ball_pitches"] for data in types.values())
        three = sum(data["counts_reaching_three_balls"] for data in types.values())
        summaries.append({
            "pitch_number_min": band + 1,
            "pitch_number_max": band + 25 if band < 100 else None,
            "pitches": sum(data["pitches"] for data in types.values()),
            "two_ball_pitches": two,
            "counts_reaching_three_balls": three,
            "three_ball_transitions_per_two_ball_pitch": ratio(three, two),
            "by_pitch_type": pitch_types,
        })
    return summaries


def pitch_summary(rows):
    pitches = defaultdict(int)
    aims = defaultdict(lambda: [0, 0.0, 0.0])
    bands = defaultdict(lambda: defaultdict(lambda: defaultdict(int)))
    for row in rows:
        _, zone, aim_x, aim_y, x, y, before, after, result, swung, pitch_type, speed, number = row
        reached_three = before == 2 and after == 3
        finished_appearance = result in TERMINAL_RESULTS
        band = bands[min(((number - 1) // 25) * 25, 100)][pitch_type]
        band["pitches"] += 1
        band["speed_sum"] += speed
        # The Center target is half the width and height of the four corner targets.
        zone_size = 0.5 if zone == "Center" else 1.0
        band["error_sq_sum"] += ((x - aim_x) ** 2 + (y - aim_y) ** 2) / (2.0 * zone_size ** 2)
        band["two_ball_pitches"] += before == 2
        band["counts_reaching_three_balls"] += reached_three
        pitches["pitches"] += 1
        pitches["aimed_in_zone"] += abs(aim_x) <= 1 and abs(aim_y) <= 1
        outside = abs(x) > 1 or abs(y) > 1
        pitches["in_zone"] += not outside
        pitches["counts_reaching_three_balls"] += reached_three
        pitches["batters_faced"] += finished_appearance
        three_balls = before == 3
        pitches["three_ball_pitches"] += three_balls
        pitches["three_ball_outside"] += three_balls and outside
        pitches["three_ball_outside_with_decision"] += (
            three_balls and outside and swung is not None
        )
        pitches["three_ball_outside_swings"] += three_balls and outside and swung == 1
        aim = aims[zone, aim_x, aim_y]
        aim[0] += 1
        aim[1] += (x - aim_x) ** 2
        aim[2] += (y - aim_y) ** 2
    pitches["aimed_in_zone_rate"] = ratio(pitches["aimed_in_zone"], pitches["pitches"])
    pitches["in_zone_rate"] = ratio(pitches["in_zone"], pitches["pitches"])
    pitches["reach_three_balls_per_bf"] = ratio(
        pitches["counts_reaching_three_balls"], pitches["batters_faced"]
    )
    pitches["three_ball_outside_rate"] = ratio(
        pitches["three_ball_outside"], pitches["three_ball_pitches"]
    )
    pitches["three_ball_outside_swing_rate"] = ratio(
        pitches["three_ball_outside_swings"], pitches["three_ball_outside_with_decision"]
    )
    return {
        **pitches,
        "by_pitch_count": pitch_count_summary(bands),
        "by_aim": [
            {"zone": zone, "x": x, "y": y, "pitches": n,
             "rms_error_x": math.sqrt(dx / n), "rms_error_y": math.sqrt(dy / n)}
            for (zone, x, y), (n, dx, dy) in sorted(aims.items())
        ],
    }


def report(conn, season):
    completed = conn.execute(COMPLETED, (season,)).fetchall()
    if not completed:
        raise ValueError(f"No completed regular-season games for {season}")
    team_games = defaultdict(int)
    for _, home, away in completed:
        team_games[home] += 1
        team_games[away] += 1

    stats = conn.execute(f"""
        WITH completed AS ({COMPLETED})
        SELECT c.player_id, p.team_id, p.first_name, p.last_name, i.control,
               SUM(c.outs), SUM(c.earned_runs), SUM(c.bb), SUM(c.so),
               SUM(c.hits), SUM(c.home_runs)
        FROM game_pitching_stat_cache AS c
        JOIN completed AS g ON g.id = c.game_id
        JOIN player_info AS p ON p.id = c.player_id
        LEFT JOIN pitcher_info AS i ON i.player_id = c.player_id
        GROUP BY c.player_id
    """, (season,)).fetchall()
    cached_games = conn.execute(f"""
        WITH completed AS ({COMPLETED})
        SELECT COUNT(DISTINCT c.game_id) FROM game_pitching_stat_cache AS c
        JOIN completed AS g ON g.id = c.game_id
    """, (season,)).fetchone()[0]
    if cached_games != len(completed):
        raise ValueError("Pitching statistics are incomplete. Open the pitching statistics screen to refresh the cache.")

    by_pitcher = defaultdict(list)
    for row in conn.execute(f"""
        WITH completed AS ({COMPLETED}),
        pitch_order AS (
            SELECT p.*, ROW_NUMBER() OVER (
                PARTITION BY p.game_id, p.pitcher_id ORDER BY p.count_seq
            ) AS pitch_number
            FROM player_game_pitching AS p JOIN completed AS g ON g.id = p.game_id
        ),
        counts AS (
            SELECT c.game_id, c.seq, c.ball,
                   LAG(c.ball, 1, 0) OVER (
                       PARTITION BY c.game_id, c.inning_seq, c.inning_tb ORDER BY c.seq
                   ) AS before_balls
            FROM count AS c JOIN completed AS g ON g.id = c.game_id
        )
        {PITCHES}
    """, (season,)):
        by_pitcher[row[0]].append(row)

    players = []
    teams = defaultdict(lambda: {"outs": 0, "earned_runs": 0})
    for player_id, team, first, last, control, outs, er, bb, so, hits, hr in stats:
        if team in team_games:
            teams[team]["outs"] += outs
            teams[team]["earned_runs"] += er
        pitch = pitch_summary(by_pitcher[player_id])
        bf = pitch["batters_faced"]
        players.append({
            "player_id": player_id, "team_id": team, "name": f"{first} {last}",
            "control": control, "qualified": team in team_games and outs >= 3 * team_games[team],
            "outs": outs, "earned_runs": er, "era": ratio(27 * er, outs),
            "walks": bb, "bb_rate": ratio(bb, bf),
            "strikeouts": so, "k_rate": ratio(so, bf),
            "hits": hits, "hit_rate": ratio(hits, bf),
            "home_runs": hr, "hr_rate": ratio(hr, bf),
            **pitch,
        })
    players.sort(key=lambda p: (p["era"] is None, p["era"], p["player_id"]))
    qualified = [p for p in players if p["qualified"]]
    total_bf = sum(p["batters_faced"] for p in players)
    total_bb = sum(p["walks"] for p in players)
    return {
        "season": season, "completed_games": len(completed), "team_games": dict(team_games),
        "league": {"bf": total_bf, "bb": total_bb, "bb_rate": ratio(total_bb, total_bf)},
        "team_era_by_current_roster": {
            team: ratio(27 * values["earned_runs"], values["outs"])
            for team, values in teams.items()
        },
        "qualified_pitchers": len(qualified),
        "qualified_zero_walk_ids": [p["player_id"] for p in qualified if p["walks"] == 0],
        "qualified_era_under_1_ids": [p["player_id"] for p in qualified if p["era"] < 1],
        "qualified_era_1_to_2_ids": [p["player_id"] for p in qualified if 1 <= p["era"] < 2],
        "pitchers": players,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--season", type=int, required=True)
    args = parser.parse_args()
    try:
        uri = db_from_env().resolve().as_uri() + "?mode=ro"
        with sqlite3.connect(uri, uri=True) as conn:
            conn.execute("BEGIN")
            print(json.dumps(report(conn, args.season), ensure_ascii=True, indent=2))
    except (sqlite3.Error, ValueError) as err:
        parser.exit(1, f"{err}\n")


if __name__ == "__main__":
    main()
