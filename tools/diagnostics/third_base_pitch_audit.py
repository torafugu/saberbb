"""Measure pitch calls and plate-appearance outcomes by third-base occupancy."""

import argparse
import json
import sqlite3
from collections import Counter, defaultdict

from diagnostic_db import db_from_env
from pitch_control_audit import COMPLETED, TERMINAL_RESULTS, ratio, saved_aim_margin
from wild_pitch_advancement_audit import replay_game, wild_pitch_advancement
from wild_pitch_audit import catcher_blocked_column, crossed_wild_pitch_limits, is_wild_ball_four


HITS = {"Single", "Double", "Triple", "HomeRun"}
LOW_AIMS = ("low_out", "low_edge", "low_border", "low_wide")


def report(conn, season, pitcher):
    games = conn.execute(COMPLETED, (season,)).fetchall()
    if not games:
        raise ValueError(f"No completed regular-season games for {season}")
    groups = defaultdict(Counter)
    blocked_column = catcher_blocked_column(conn)
    for game, *_ in games:
        pitches = {
            seq: (zone, ax, ay, x, y, batter, result, points, blocked)
            for seq, zone, ax, ay, x, y, batter, result, points, blocked in conn.execute(f"""
                SELECT p.count_seq, p.aim_zone, p.aim_location_x, p.aim_location_y,
                       p.actual_location_x, p.actual_location_y, b.batter_id, b.result, c.point, {blocked_column}
                FROM player_game_pitching p
                LEFT JOIN player_game_batting b ON b.game_id=p.game_id AND b.count_seq=p.count_seq
                LEFT JOIN count c ON c.game_id=p.game_id AND c.seq=p.count_seq
                WHERE p.game_id=? AND (? IS NULL OR p.pitcher_id=?)
            """, (game, pitcher, pitcher))
        }
        if not pitches:
            continue

        def observe(seq, bases):
            pitch = pitches.pop(seq, None)
            if pitch is None:
                return
            zone, ax, ay, x, y, batter, result, points, blocked = pitch
            if bases is None:
                key = "unknown"
            else:
                key = "runner_on_third" if bases[2] is not None else "no_runner_on_third"
            n = groups[key]
            n["pitches"] += 1
            if ay < 0:
                n[f"low_{saved_aim_margin(zone, ax, ay).lower()}"] += 1
            n["plate_appearances"] += result in TERMINAL_RESULTS
            n["walks"] += result == "Walk"
            n["strikeouts"] += result == "Strikeout"
            n["hits"] += result in HITS
            n["home_runs"] += result == "HomeRun"
            limits = crossed_wild_pitch_limits(x, y)
            n["below_wild_limit"] += "low" in limits
            ball_four = is_wild_ball_four(result, limits, blocked)
            if result == "WildPitch" or ball_four:
                advance = wild_pitch_advancement(bases, batter, {"ball_four": ball_four}, points)
                if advance["advancement"] in {"runner_advanced", "extra_advance_beyond_walk"}:
                    n["scored_wild_pitches"] += 1
                    forced_run = ball_four and all(r is not None for r in bases)
                    n["runs_on_wild_pitches"] += points - int(forced_run)

        replay_game(conn, game, {}, observe)
        groups["unknown"]["pitches"] += len(pitches)
    if not sum(group["pitches"] for group in groups.values()):
        raise ValueError(f"No pitching records for player {pitcher} in regular season {season}")
    summary = {}
    for label in ("runner_on_third", "no_runner_on_third", "unknown"):
        n = groups[label]
        summary[label] = {key: n[key] for key in (
            "pitches", *LOW_AIMS, "below_wild_limit", "scored_wild_pitches", "runs_on_wild_pitches",
            "plate_appearances", "walks", "strikeouts", "hits", "home_runs",
        )}
        for key in LOW_AIMS:
            summary[label][f"{key}_rate"] = ratio(n[key], n["pitches"])
        for key in ("below_wild_limit", "scored_wild_pitches", "runs_on_wild_pitches"):
            summary[label][f"{key}_per_1000"] = ratio(1000 * n[key], n["pitches"])
        for key in ("walks", "strikeouts", "hits", "home_runs"):
            summary[label][f"{key}_per_pa"] = ratio(n[key], n["plate_appearances"])
    return {"season": season, "completed_games": len(games), "pitcher_id": pitcher, **summary}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--season", type=int, required=True)
    parser.add_argument("--pitcher-id", type=int)
    args = parser.parse_args()
    try:
        uri = db_from_env().resolve().as_uri() + "?mode=ro"
        with sqlite3.connect(uri, uri=True) as conn:
            conn.execute("BEGIN")
            print(json.dumps(report(conn, args.season, args.pitcher_id), indent=2))
    except (sqlite3.Error, ValueError) as err:
        parser.exit(1, f"{err}\n")


if __name__ == "__main__":
    main()
