"""Trace regular-season wild pitches to saved pitch locations and results."""

import argparse
import json
import sqlite3
from collections import Counter, defaultdict

from diagnostic_db import db_from_env
from pitch_control_audit import COMPLETED, ratio, saved_aim_margin


def crossed_wild_pitch_limits(x, y):
    # Match BallLocation::call, including its strict comparisons.
    return [
        label for label, crossed in (
            ("horizontal", abs(x) > 3.2),
            ("high", y > 3.5),
            ("low", y < -2.2),
        ) if crossed
    ]


def summarize_groups(groups):
    return {
        label: {
            "pitches": n["pitches"],
            "extreme_locations": n["extreme_locations"],
            "wild_pitches": n["wild_pitches"],
            "wild_pitches_per_1000": ratio(1000 * n["wild_pitches"], n["pitches"]),
        }
        for label, n in sorted(groups.items())
    }


def report(conn, season):
    games = conn.execute(COMPLETED, (season,)).fetchall()
    if not games:
        raise ValueError(f"No completed regular-season games for {season}")
    pitches = defaultdict(Counter)
    by_type = defaultdict(lambda: defaultdict(Counter))
    by_aim = defaultdict(lambda: defaultdict(Counter))
    by_workload = defaultdict(lambda: defaultdict(Counter))
    events = defaultdict(list)
    for row in conn.execute(f"""
        WITH completed AS ({COMPLETED}), pitch_order AS (
            SELECT p.*, ROW_NUMBER() OVER (
                PARTITION BY p.game_id, p.pitcher_id ORDER BY p.count_seq
            ) AS pitch_number
            FROM player_game_pitching p JOIN completed g ON g.id = p.game_id
        ), counts AS (
            SELECT c.game_id, c.seq, c.inning_seq, c.inning_tb, c.point,
                   LAG(c.ball, 1, 0) OVER half_inning AS before_balls,
                   LAG(c.strike, 1, 0) OVER half_inning AS before_strikes
            FROM count c JOIN completed g ON g.id = c.game_id
            WINDOW half_inning AS (
                PARTITION BY c.game_id, c.inning_seq, c.inning_tb ORDER BY c.seq
            )
        )
        SELECT p.pitcher_id, p.game_id, p.count_seq, p.pitch_type, p.pitch_number,
               p.aim_zone, p.aim_location_x, p.aim_location_y,
               p.actual_location_x, p.actual_location_y, b.result, b.decision_swung,
               c.inning_seq, c.inning_tb, c.before_balls, c.before_strikes, c.point
        FROM pitch_order p
        LEFT JOIN player_game_batting b ON b.game_id = p.game_id AND b.count_seq = p.count_seq
        LEFT JOIN counts c ON c.game_id = p.game_id AND c.seq = p.count_seq
        ORDER BY p.game_id, p.count_seq
    """, (season,)):
        (pitcher, game, seq, pitch_type, number, zone, aim_x, aim_y, x, y,
         result, swung, inning, half, balls, strikes, runs) = row
        limits = crossed_wild_pitch_limits(x, y)
        wild_ball_four = result == "Walk" and bool(limits)
        recorded = result == "WildPitch" or wild_ball_four
        margin = saved_aim_margin(zone, aim_x, aim_y)
        band_start = (number - 1) // 25 * 25 + 1
        workload = f"{band_start:03d}-{band_start + 24:03d}"
        for group in (pitches[pitcher], by_type[pitcher][pitch_type],
                      by_aim[pitcher][f"{zone}/{margin}"], by_workload[pitcher][workload]):
            group["pitches"] += 1
            group["extreme_locations"] += bool(limits)
            group["wild_pitches"] += recorded
        if recorded:
            events[pitcher].append({
                "game_id": game, "count_seq": seq, "inning": inning, "half": half,
                "pitch_type": pitch_type, "pitch_number": number, "aim_zone": zone,
                "aim_margin": margin, "aim_x": aim_x, "aim_y": aim_y,
                "before_balls": balls, "before_strikes": strikes, "recorded_result": result,
                "actual_x": x, "actual_y": y, "error_x": x - aim_x, "error_y": y - aim_y,
                "limits_crossed": limits, "ball_four": wild_ball_four,
                "swung": None if swung is None else bool(swung), "runs_on_pitch": runs,
            })

    stats = {
        p: (outs, wild) for p, outs, wild in conn.execute(f"""
            WITH completed AS ({COMPLETED})
            SELECT c.player_id, SUM(c.outs), SUM(c.wild_pitches)
            FROM game_pitching_stat_cache c JOIN completed g ON g.id = c.game_id
            GROUP BY c.player_id
        """, (season,))
    }
    names = dict(conn.execute("SELECT id, first_name || ' ' || last_name FROM player_info"))
    players = []
    for pitcher in pitches.keys() | stats.keys():
        outs, cached = stats.get(pitcher, (None, None))
        totals = summarize_groups({"total": pitches[pitcher]})["total"]
        players.append({
            "player_id": pitcher, "name": names.get(pitcher), "outs": outs,
            **totals,
            "wild_pitches_per_9_ip": ratio(27 * totals["wild_pitches"], outs),
            "cached_wild_pitches": cached,
            "cache_difference": cached - totals["wild_pitches"] if cached is not None else None,
            "by_pitch_type": summarize_groups(by_type[pitcher]),
            "by_aim_zone_and_margin": summarize_groups(by_aim[pitcher]),
            "by_appearance_pitch_number": summarize_groups(by_workload[pitcher]),
            "events": events[pitcher],
        })
    players.sort(key=lambda p: (-max(p["wild_pitches"], p["cached_wild_pitches"] or 0), p["player_id"]))
    league = Counter()
    for totals in pitches.values():
        league.update(totals)
    cache_state_exists = conn.execute("""
        SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'stat_cache_state'
    """).fetchone()
    cache_state = conn.execute(
        "SELECT version FROM stat_cache_state WHERE id = 1 AND ready = 1"
    ).fetchone() if cache_state_exists else None
    return {
        "season": season, "completed_games": len(games),
        "cache_version": cache_state[0] if cache_state else None,
        "league": summarize_groups({"total": league})["total"], "pitchers": players,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--season", type=int, required=True)
    args = parser.parse_args()
    try:
        uri = db_from_env().resolve().as_uri() + "?mode=ro"
        with sqlite3.connect(uri, uri=True) as conn:
            conn.execute("BEGIN")
            print(json.dumps(report(conn, args.season), indent=2))
    except (sqlite3.Error, ValueError) as err:
        parser.exit(1, f"{err}\n")


if __name__ == "__main__":
    main()
