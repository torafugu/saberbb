"""Report saved regular-season pitching control and walk rates as JSON."""

import argparse
from bisect import bisect_left
import json
import math
import sqlite3
from collections import Counter, defaultdict

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
           p.pitch_type, p.speed, p.pitch_number, c.before_strikes, b.launch_speed
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



# PitchCall::aim_location stores coordinates rather than the selected margin.
CORNER_AIMS = {"Wide": 1.0 - 1.0 / 3.0, "Edge": 0.75, "Out": 1.2}
FAIR_CONTACT_RESULTS = TERMINAL_RESULTS - {"Walk", "Strikeout", "HitByPitch"}
HARD_HIT_SPEED_MS = 42.4688  # 95 mph
OUTSIDE_DISTANCE_LIMITS = (0.05, 0.10, 0.25, 0.50)


def saved_aim_margin(zone, x, y):
    if zone == "Center" and x == 0.0 and y == 0.0:
        return "Center"
    if zone in ("LowInside", "LowOutside", "HighInside", "HighOutside"):
        near, far = sorted((abs(x), abs(y)))
        if math.isclose(near, CORNER_AIMS["Wide"], rel_tol=0.0, abs_tol=1e-6) and math.isclose(
            far, 0.98, rel_tol=0.0, abs_tol=1e-6
        ):
            return "Border"
        for margin, magnitude in CORNER_AIMS.items():
            if math.isclose(abs(x), magnitude, rel_tol=0.0, abs_tol=1e-6) and math.isclose(
                abs(y), magnitude, rel_tol=0.0, abs_tol=1e-6
            ):
                return margin
    return "Other"


def saved_pitch_outcome(result, swung, advances_ball_count):
    if result == "Take":
        return "called_ball" if advances_ball_count else "called_strike"
    if result == "StrikeSwung":
        return "swinging_strike"
    if result == "Strikeout":
        if swung is None:
            return "strikeout_unknown_swing"
        return "swinging_strikeout" if swung else "called_strikeout"
    if result in FAIR_CONTACT_RESULTS:
        return "fair_contact"
    return {
        "Foul": "foul", "Walk": "walk", "WildPitch": "wild_pitch", "HitByPitch": "hit_by_pitch",
    }.get(result, "unrecorded_or_other")


def summarize_count_margins(rows):
    counts = {}
    for (balls, strikes, margin), series in sorted(rows.items()):
        outcomes = {"in_zone_outcomes": Counter(), "out_of_zone_outcomes": Counter()}
        actual_outside = advanced = hard_hit = 0
        for (outside, advances_ball_count, outcome, hard_hit_fair), n in sorted(series.items()):
            outcomes["out_of_zone_outcomes" if outside else "in_zone_outcomes"][outcome] += n
            actual_outside += n if outside else 0
            advanced += n if advances_ball_count else 0
            hard_hit += n if hard_hit_fair else 0
        pitches = sum(series.values())
        counts.setdefault(f"{balls}-{strikes}", {})[margin] = {
            "pitches": pitches,
            "actual_outside": actual_outside,
            "actual_outside_rate": ratio(actual_outside, pitches),
            "ball_count_advances": advanced,
            "ball_count_advance_rate": ratio(advanced, pitches),
            "hard_hit_fair": hard_hit,
            **outcomes,
        }
    return counts


def summarize_count_endings(rows):
    by_count = defaultdict(Counter)
    for (balls, strikes, _margin), series in rows.items():
        count = by_count[balls, strikes]
        for (_outside, advances, outcome, _hard_hit_fair), n in series.items():
            count["pitches"] += n
            count["ball_count_advances_without_walk"] += n if advances and outcome != "walk" else 0
            count[outcome] += n
    report = {}
    for (balls, strikes), count in sorted(by_count.items()):
        strikeouts = count["called_strikeout"] + count["swinging_strikeout"] + count["strikeout_unknown_swing"]
        report[f"{balls}-{strikes}"] = {
            "pitches": count["pitches"],
            "ball_count_advances_without_walk": count["ball_count_advances_without_walk"],
            "finished_appearances": strikeouts + count["fair_contact"] + count["walk"] + count["hit_by_pitch"],
            "strikeouts": strikeouts,
            "called_strikeouts": count["called_strikeout"],
            "swinging_strikeouts": count["swinging_strikeout"],
            "strikeouts_unknown_swing": count["strikeout_unknown_swing"],
            "fair_contact": count["fair_contact"],
            "walks": count["walk"],
            "hit_by_pitch": count["hit_by_pitch"],
        }
    return report


def summarize_outside_distances(data):
    rows = []
    lower_limits = (0.0, *OUTSIDE_DISTANCE_LIMITS)
    upper_limits = (*OUTSIDE_DISTANCE_LIMITS, None)
    for band, (lower, upper) in enumerate(zip(lower_limits, upper_limits)):
        swings = data[band, "swings"]
        takes = data[band, "takes"]
        unknown = data[band, "unknown_decisions"]
        pitches = swings + takes + unknown
        if pitches:
            rows.append({
                "distance_min_exclusive": lower,
                "distance_max_inclusive": upper,
                "pitches": pitches,
                "swings": swings,
                "takes": takes,
                "unknown_decisions": unknown,
                "swing_rate": ratio(swings, swings + takes),
                "advances_to_three_balls": data[band, "advances_to_three_balls"],
            })
    return rows


def summarize_count_locations(locations):
    rows = []
    for (balls, strikes, zone, aim_x, aim_y), data in sorted(locations.items()):
        row = {
            "count": f"{balls}-{strikes}",
            "zone": zone,
            "margin": saved_aim_margin(zone, aim_x, aim_y),
            "aim_x": aim_x,
            "aim_y": aim_y,
            "aim_depth": 1.0 - max(abs(aim_x), abs(aim_y)),
            "pitches": data["pitches"],
            "actual_outside": data["actual_outside"],
            "actual_outside_rate": ratio(data["actual_outside"], data["pitches"]),
            "outside_swings": data["outside_swings"],
            "outside_takes": data["outside_takes"],
            "outside_unknown_decisions": data["outside_unknown_decisions"],
            "mean_error_x": data["error_x"] / data["pitches"],
            "mean_error_y": data["error_y"] / data["pitches"],
            "rms_error_x": math.sqrt(data["error_sq_x"] / data["pitches"]),
            "rms_error_y": math.sqrt(data["error_sq_y"] / data["pitches"]),
        }
        if balls == 3:
            row["walks"] = data["walks"]
        else:
            advances = data["advances_to_three_balls"]
            row["advances_to_three_balls"] = advances
            row["advance_to_three_balls_rate"] = ratio(advances, data["pitches"])
            row["outside_by_distance"] = summarize_outside_distances(data)
        rows.append(row)
    return rows


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


def zero_two_summary(counts):
    return {
        **counts,
        "aimed_outside_rate": ratio(counts["aimed_outside"], counts["pitches"]),
        "actual_outside_rate": ratio(counts["actual_outside"], counts["pitches"]),
    }


def pitch_summary(rows, count_margins=None, count_locations=None):
    zero_two = dict.fromkeys(("pitches", "aimed_outside", "actual_outside"), 0)
    pitches = defaultdict(int)
    aims = defaultdict(lambda: [0, 0.0, 0.0])
    bands = defaultdict(lambda: defaultdict(lambda: defaultdict(int)))
    for row in rows:
        (_, zone, aim_x, aim_y, x, y, before, after, result, swung,
         pitch_type, speed, number, strikes, launch_speed) = row
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
        aimed_outside = abs(aim_x) > 1 or abs(aim_y) > 1
        outside = abs(x) > 1 or abs(y) > 1
        pitches["aimed_in_zone"] += not aimed_outside
        if count_margins is not None:
            margin = saved_aim_margin(zone, aim_x, aim_y)
            advances = after == before + 1 or result == "Walk"
            outcome = saved_pitch_outcome(result, swung, advances)
            hard_hit = outcome == "fair_contact" and launch_speed is not None and launch_speed >= HARD_HIT_SPEED_MS
            count_margins[before, strikes, margin][outside, advances, outcome, hard_hit] += 1
        if before == 0 and strikes == 2:
            zero_two["pitches"] += 1
            zero_two["aimed_outside"] += aimed_outside
            zero_two["actual_outside"] += outside
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
        if count_locations and (before, strikes) in count_locations:
            data = count_locations[before, strikes][before, strikes, zone, aim_x, aim_y]
            dx, dy = x - aim_x, y - aim_y
            data["pitches"] += 1
            data["actual_outside"] += outside
            data["outside_swings"] += outside and swung == 1
            data["outside_takes"] += outside and swung == 0
            data["outside_unknown_decisions"] += outside and swung is None
            data["walks"] += result == "Walk"
            data["advances_to_three_balls"] += reached_three
            if outside and before == 2 and strikes == 2:
                # Match BallLocation::distance_from_zone_edge at the sides and corners.
                distance = math.hypot(max(abs(x) - 1.0, 0.0), max(abs(y) - 1.0, 0.0))
                band = bisect_left(OUTSIDE_DISTANCE_LIMITS, distance)
                decision = {1: "swings", 0: "takes"}.get(swung, "unknown_decisions")
                data[band, decision] += 1
                data[band, "advances_to_three_balls"] += reached_three
            data["error_x"] += dx
            data["error_y"] += dy
            data["error_sq_x"] += dx ** 2
            data["error_sq_y"] += dy ** 2
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
        "zero_two": zero_two_summary(zero_two),
        "by_pitch_count": pitch_count_summary(bands),
        "by_aim": [
            {"zone": zone, "x": x, "y": y, "pitches": n,
             "rms_error_x": math.sqrt(dx / n), "rms_error_y": math.sqrt(dy / n)}
            for (zone, x, y), (n, dx, dy) in sorted(aims.items())
        ],
    }


def report(
    conn, season, include_count_margins=False, include_three_ball_locations=False,
    include_count_endings=False, include_two_two_locations=False,
):
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
                   ) AS before_balls,
                   LAG(c.strike, 1, 0) OVER (
                       PARTITION BY c.game_id, c.inning_seq, c.inning_tb ORDER BY c.seq
                   ) AS before_strikes
            FROM count AS c JOIN completed AS g ON g.id = c.game_id
        )
        {PITCHES}
    """, (season,)):
        by_pitcher[row[0]].append(row)

    players = []
    league_count_margins = defaultdict(Counter)
    location_sections = {}
    if include_three_ball_locations:
        location_sections["three_ball_locations"] = ((3, 0), (3, 1), (3, 2))
    if include_two_two_locations:
        location_sections["two_two_locations"] = ((2, 2),)
    league_locations = {section: defaultdict(Counter) for section in location_sections}
    teams = defaultdict(lambda: {"outs": 0, "earned_runs": 0})
    for player_id, team, first, last, control, outs, er, bb, so, hits, hr in stats:
        if team in team_games:
            teams[team]["outs"] += outs
            teams[team]["earned_runs"] += er
        count_margins = defaultdict(Counter) if include_count_margins or include_count_endings else None
        locations = {section: defaultdict(Counter) for section in location_sections}
        count_locations = {
            count: locations[section]
            for section, counts in location_sections.items()
            for count in counts
        }
        pitch = pitch_summary(by_pitcher[player_id], count_margins, count_locations)
        if count_margins is not None:
            for key, series in count_margins.items():
                league_count_margins[key].update(series)
            if include_count_margins:
                pitch["by_count_and_margin"] = summarize_count_margins(count_margins)
            if include_count_endings:
                pitch["by_count_endings"] = summarize_count_endings(count_margins)
        for section, rows in locations.items():
            for key, data in rows.items():
                league_locations[section][key].update(data)
            pitch[section] = summarize_count_locations(rows)
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
    league = {
        "bf": total_bf, "bb": total_bb, "bb_rate": ratio(total_bb, total_bf),
        "zero_two": zero_two_summary({
            key: sum(p["zero_two"][key] for p in players)
            for key in ("pitches", "aimed_outside", "actual_outside")
        }),
    }
    if include_count_margins:
        league["by_count_and_margin"] = summarize_count_margins(league_count_margins)
    if include_count_endings:
        league["by_count_endings"] = summarize_count_endings(league_count_margins)
    for section, rows in league_locations.items():
        league[section] = summarize_count_locations(rows)
    return {
        "season": season, "completed_games": len(completed), "team_games": dict(team_games),
        "league": league,
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
    parser.add_argument(
        "--require-zero-two-out", action="store_true",
        help="Fail if no saved 0–2 pitch was aimed outside the strike zone.",
    )
    parser.add_argument(
        "--count-margins", action="store_true",
        help="Report actual zone location and outcomes by pre-pitch count and saved aim margin.",
    )
    parser.add_argument(
        "--count-endings", action="store_true",
        help="Report plate appearance endings and ball-count advances by pre-pitch count.",
    )
    parser.add_argument(
        "--three-ball-locations", action="store_true",
        help="Report actual location errors and batter decisions by three-ball count and exact saved aim.",
    )
    parser.add_argument(
        "--two-two-locations", action="store_true",
        help="Report 2–2 location errors, batter decisions, and advances to 3–2 by exact saved aim.",
    )
    args = parser.parse_args()
    try:
        uri = db_from_env().resolve().as_uri() + "?mode=ro"
        with sqlite3.connect(uri, uri=True) as conn:
            conn.execute("BEGIN")
            output = report(
                conn, args.season, include_count_margins=args.count_margins,
                include_three_ball_locations=args.three_ball_locations,
                include_count_endings=args.count_endings,
                include_two_two_locations=args.two_two_locations,
            )
            if args.require_zero_two_out and output["league"]["zero_two"]["aimed_outside"] == 0:
                raise ValueError(
                    "No saved 0–2 pitch was aimed outside the strike zone. "
                    "Check simulation.pitch_around_0_2_edge_scale = 0.0 in default-config.toml during game processing."
                )
            print(json.dumps(output, ensure_ascii=True, indent=2))
    except (sqlite3.Error, ValueError) as err:
        parser.exit(1, f"{err}\n")


if __name__ == "__main__":
    main()
