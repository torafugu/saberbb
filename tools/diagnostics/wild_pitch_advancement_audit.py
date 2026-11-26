"""Audit which recorded wild pitches led to advancement in saved games."""

import argparse
import json
import sqlite3
from collections import Counter, defaultdict

from diagnostic_db import db_from_env
from wild_pitch_audit import crossed_wild_pitch_limits, report as wild_pitch_report


EMPTY_BASES = (None, None, None)
UNCHANGED_BASES = {"Take", "StrikeSwung", "Strikeout", "Foul", "Out"}


def walk_advance(bases, batter):
    first, second, third = bases
    return (batter, first if first is not None else second,
            second if first is not None and second is not None else third)


def wild_pitch_advancement(bases, batter, event, points):
    unknown = {"advancement": "unknown", "runners_before": bases}
    if bases is None or points != int(bases[2] is not None):
        return unknown

    ball_four = event["ball_four"]
    after = (batter if ball_four else None, bases[0], bases[1])
    if ball_four:
        if batter is None:
            return unknown
        normal_walk = walk_advance(bases, batter)
        # An existing runner absent from the ordinary-walk bases scored on the walk.
        extra = [
            runner for destination, runner in enumerate(bases, start=2)
            if runner is not None and runner in normal_walk
            and destination > normal_walk.index(runner) + 1
        ]
        category = "extra_advance_beyond_walk" if extra else (
            "forced_by_walk_only" if any(bases) else "bases_empty"
        )
    else:
        extra = [runner for runner in bases if runner is not None]
        category = "runner_advanced" if extra else "bases_empty"
    return {
        "advancement": category,
        "runners_before": bases,
        "runners_after": after,
        "runner_ids_advanced_by_wild_pitch": extra,
    }


def replay_game(conn, game, events):
    """Follow the saved plays using the same transitions as the batting stat cache."""
    bases = None
    half = None
    last_seq = None
    for row in conn.execute("""
        SELECT c.seq, c.inning_seq, c.inning_tb, c.point, b.batter_id, b.result,
               p.actual_location_x, p.actual_location_y,
               s.event, s.runner_1st_id, s.runner_2nd_id, s.runner_3rd_id,
               r.event, r.runner_1st_id, r.runner_2nd_id, r.runner_3rd_id
        FROM count c
        LEFT JOIN player_game_batting b ON b.game_id = c.game_id AND b.count_seq = c.seq
        LEFT JOIN player_game_pitching p ON p.game_id = c.game_id AND p.count_seq = c.seq
        LEFT JOIN player_game_running s ON s.game_id = c.game_id AND s.count_seq = c.seq
                                       AND s.event = 'BaseSteal'
        LEFT JOIN player_game_running r ON r.game_id = c.game_id AND r.count_seq = c.seq
            AND r.seq = (SELECT MAX(seq) FROM player_game_running
                         WHERE game_id = c.game_id AND count_seq = c.seq AND event != 'BaseSteal')
        WHERE c.game_id = ?
        ORDER BY c.seq
    """, (game,)):
        (seq, inning, side, points, batter, result, x, y,
         steal, s_first, s_second, s_third, running, r_first, r_second, r_third) = row
        if (inning, side) != half:
            bases = EMPTY_BASES
            half = (inning, side)
        if last_seq is not None and seq != last_seq + 1:
            bases = None
        last_seq = seq
        if steal:
            # The steal snapshot precedes any batted-ball outcome on the same count.
            # Failed steals leave the runner in place in the current simulator.
            bases = (s_first, s_second, s_third)
        ball_four = result == "Walk" and x is not None and bool(crossed_wild_pitch_limits(x, y))
        if seq in events:
            events[seq].update(wild_pitch_advancement(bases, batter, events[seq], points))

        if result == "HomeRun":
            bases = EMPTY_BASES
        elif running in {"GrounderPlay", "DoublePlay"}:
            bases = (r_first, r_second, r_third)
        elif bases is None:
            continue
        elif result == "WildPitch" or ball_four:
            bases = (batter if ball_four else None, bases[0], bases[1]) if (
                points == int(bases[2] is not None) and (not ball_four or batter is not None)
            ) else None
        elif result in {"Walk", "HitByPitch"}:
            bases = walk_advance(bases, batter) if (
                batter is not None and points == int(all(r is not None for r in bases))
            ) else None
        elif result == "Double" and running is None:
            # Ground-rule doubles have no running snapshot.
            bases = (None, batter, bases[0]) if (
                batter is not None and points == sum(r is not None for r in bases[1:])
            ) else None
        elif running == "TagUp" or (result in UNCHANGED_BASES and points == 0):
            # Tag-up snapshots are logged, but the simulator does not commit them.
            continue
        elif result is not None or points != 0 or (x is not None and not steal):
            bases = None


def report(conn, season, pitcher_id):
    source = wild_pitch_report(conn, season)
    pitchers = [p for p in source["pitchers"] if pitcher_id is None or p["player_id"] == pitcher_id]
    if not pitchers:
        raise ValueError(f"No pitching records for player {pitcher_id} in regular season {season}")

    games = defaultdict(dict)
    for pitcher in pitchers:
        for event in pitcher["events"]:
            event["advancement"] = "unknown"
            games[event["game_id"]][event["count_seq"]] = event
    for game, events in sorted(games.items()):
        replay_game(conn, game, events)

    results = []
    season_totals = Counter()
    categories = ("runner_advanced", "extra_advance_beyond_walk", "forced_by_walk_only",
                  "bases_empty", "unknown")
    for pitcher in pitchers:
        counts = Counter(event["advancement"] for event in pitcher["events"])
        season_totals.update(counts)
        results.append({
            "player_id": pitcher["player_id"], "name": pitcher["name"], "outs": pitcher["outs"],
            "recorded_wild_pitches": pitcher["wild_pitches"],
            "cached_wild_pitches": pitcher["cached_wild_pitches"],
            "cache_difference": pitcher["cache_difference"],
            "advancement": {key: counts[key] for key in categories},
            "with_wild_pitch_advancement": counts["runner_advanced"] + counts["extra_advance_beyond_walk"],
            "events": pitcher["events"],
        })
    return {
        "season": season, "completed_games": source["completed_games"], "pitcher_filter": pitcher_id,
        "advancement": {key: season_totals[key] for key in categories}, "pitchers": results,
    }


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
