"""Compare saved pitch location misses with peers and a supplied execution model."""

import argparse
import json
import math
import os
import sqlite3
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import date

from diagnostic_db import db_from_env
from pitch_control_audit import COMPLETED, ratio, saved_aim_margin
from wild_pitch_advancement_audit import replay_game, wild_pitch_advancement
from wild_pitch_audit import crossed_wild_pitch_limits


@dataclass(frozen=True)
class Settings:
    irreducible_error: float
    error_floor: float
    control_cap: float | None
    control_above_cap_scale: float
    fatigue: bool

    @classmethod
    def from_env(cls):
        def bounded(key, default, limit):
            value = float(os.environ.get(key, default))
            if not 0 <= value <= limit:
                raise ValueError(f"{key} must be between 0 and {limit}")
            return value

        cap = os.environ.get("SABERBB_PITCH_CONTROL_CAP")
        cap = float(cap) if cap else None
        if cap is not None and not math.isfinite(cap):
            raise ValueError("SABERBB_PITCH_CONTROL_CAP must be finite")
        fatigue = os.environ.get("SABERBB_PITCH_EXECUTION_FATIGUE", "0")
        if fatigue not in {"0", "1"}:
            raise ValueError("SABERBB_PITCH_EXECUTION_FATIGUE must be 0 or 1")
        return cls(
            bounded("SABERBB_PITCH_LOCATION_IRREDUCIBLE_ERROR", 0.20, 0.5),
            bounded("SABERBB_PITCH_LOCATION_ERROR_FLOOR", 0.0, 0.5),
            cap,
            bounded("SABERBB_PITCH_CONTROL_ABOVE_CAP_SCALE", 0.0, 1.0),
            fatigue == "1",
        )

    def control_components(self, control):
        effective = control
        if self.control_cap is not None and control > self.control_cap:
            if self.control_above_cap_scale == 0.0:
                effective = self.control_cap
            elif self.control_above_cap_scale < 1.0:
                effective = self.control_cap + (control - self.control_cap) * self.control_above_cap_scale
        try:
            control_std = 1.0 - 1.0 / (1.0 + math.exp(-effective))
        except OverflowError:
            control_std = 1.0
        floored_std = max(control_std, self.error_floor)
        return {
            "effective_control": effective,
            "control_std": control_std,
            "floored_std": floored_std,
            "before_fatigue_std": math.hypot(floored_std, self.irreducible_error),
        }


def workload_by_appearance(conn):
    """Reconstruct entering load from saved appearances in schedule order."""
    table_exists = conn.execute("""
        SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'game_pitcher_workload'
    """).fetchone()
    if not table_exists:
        return {}
    history = defaultdict(list)
    workloads = {}
    for game, player, pitches, reference, actual, planned, seq in conn.execute("""
        SELECT w.game_id, w.player_id, w.pitches, w.reference_pitches,
               g.actual_date, g.planned_date, COALESCE(g.seq, 0)
        FROM game_pitcher_workload w JOIN game g ON g.id = w.game_id
        WHERE g.actual_date IS NOT NULL AND g.actual_date != '1900-01-01'
        ORDER BY g.planned_date, COALESCE(g.seq, 0), g.id
    """):
        current_date = date.fromisoformat(planned)
        schedule = (planned, seq, game)
        entering = sum(
            prev_pitches * (reference / prev_reference)
            * 2 ** (-((current_date - prev_date).days) * (prev_reference / 100.0))
            for prev_date, prev_key, prev_pitches, prev_reference in history[player]
            if prev_key < schedule
        )
        workloads[game, player] = (entering, reference, pitches)
        history[player].append((date.fromisoformat(actual), (actual, seq, game), pitches, reference))
    return workloads


def low_tail_probability(aim_y, axis_size, std):
    # BallLocation::call checks y < -2.2 before official scoring.
    cutoff = (-2.2 - aim_y) / axis_size
    return math.erfc(-cutoff / (math.sqrt(2) * std)) / 2 if std else float(cutoff > 0)


def observe(totals, zone, aim_x, aim_y, x, y, result, modeled):
    axis_size = 0.5 if zone == "Center" else 1.0
    dx, dy = (x - aim_x) / axis_size, (y - aim_y) / axis_size
    limits = crossed_wild_pitch_limits(x, y)
    totals["pitches"] += 1
    totals["sum_y"] += dy
    totals["sum_x_squared"] += dx * dx
    totals["sum_y_squared"] += dy * dy
    totals["below_wild_limit"] += "low" in limits
    totals["wild_pitches"] += result == "WildPitch" or (result == "Walk" and bool(limits))
    if modeled is None:
        return
    base, fatigue_scale = modeled
    final_std = base["before_fatigue_std"] * fatigue_scale
    totals["modeled_pitches"] += 1
    totals["fatigue_scale_sum"] += fatigue_scale
    totals["modeled_std_squared"] += final_std * final_std
    totals["expected_below_limit"] += low_tail_probability(aim_y, axis_size, final_std)
    totals["expected_without_irreducible_error"] += low_tail_probability(
        aim_y, axis_size, base["floored_std"] * fatigue_scale
    )
    totals["expected_without_fatigue"] += low_tail_probability(
        aim_y, axis_size, base["before_fatigue_std"]
    )


def summarize(totals, model):
    n = totals["pitches"]
    summary = {"pitches": n}
    for key in ("below_wild_limit", "wild_pitches"):
        summary[key] = totals[key]
        summary[f"{key}_per_1000"] = ratio(1000 * totals[key], n)
    summary["mean_vertical_error"] = ratio(totals["sum_y"], n)
    for axis, label in (("x", "horizontal"), ("y", "vertical")):
        summary[f"rms_{label}_error"] = math.sqrt(totals[f"sum_{axis}_squared"] / n) if n else None
    if model:
        m = totals["modeled_pitches"]
        summary["model"] = {
            "pitches": m,
            "mean_fatigue_scale": ratio(totals["fatigue_scale_sum"], m),
            "rms_location_std": math.sqrt(totals["modeled_std_squared"] / m) if m else None,
            **{key: totals[key] if m else None for key in (
                "expected_below_limit", "expected_without_irreducible_error",
                "expected_without_fatigue",
            )},
        }
    return summary


def matches_base_state(runners, base_state):
    if base_state == "third":
        return runners[2] is not None
    return runners[2] is None and (base_state == "no-third" or any(runner is not None for runner in runners))


def report(conn, season, subject, pitch_type, settings, base_state="any"):
    abilities = {
        player: (name, control, skill_control)
        for player, name, control, skill_control in conn.execute("""
            SELECT p.id, p.first_name || ' ' || p.last_name, i.control, s.control
            FROM player_info p JOIN pitcher_info i ON i.player_id = p.id
            LEFT JOIN pitch_skill s ON s.player_id = p.id AND s.pitch_type = ?
        """, (pitch_type,))
    }
    bases = {
        player: settings.control_components(control)
        for player, (_, control, _) in abilities.items()
    } if settings else {}
    workloads = workload_by_appearance(conn) if settings and settings.fatigue else {}
    by_stratum = defaultdict(lambda: defaultdict(Counter))
    by_player = defaultdict(Counter)
    by_aim = defaultdict(lambda: defaultdict(Counter))
    population = Counter()
    unknown_base_state = Counter()
    current_game = None
    runners_by_pitch = {}
    for row in conn.execute(f"""
        WITH completed AS ({COMPLETED}), ordered AS (
            SELECT p.*, ROW_NUMBER() OVER appearance AS pitch_number,
                   COUNT(*) OVER (PARTITION BY p.game_id, p.pitcher_id) AS pitch_count
            FROM player_game_pitching p JOIN completed g ON g.id = p.game_id
            WINDOW appearance AS (PARTITION BY p.game_id, p.pitcher_id ORDER BY p.count_seq)
        )
        SELECT p.pitcher_id, p.game_id, p.count_seq, p.pitch_number, p.pitch_count, p.aim_zone,
               p.aim_location_x, p.aim_location_y, p.actual_location_x, p.actual_location_y,
               b.result, b.batter_id, c.point
        FROM ordered p
        LEFT JOIN player_game_batting b ON b.game_id = p.game_id AND b.count_seq = p.count_seq
        LEFT JOIN count c ON c.game_id = p.game_id AND c.seq = p.count_seq
        WHERE p.pitch_type = ?
        ORDER BY p.game_id, p.count_seq
    """, (season, pitch_type)):
        player, game, seq, number, pitch_count, zone, ax, ay, x, y, result, batter, points = row
        side = "subject" if player == subject else "peers"
        scored = 0
        if base_state != "any":
            if game != current_game:
                current_game = game
                runners_by_pitch.clear()
                replay_game(conn, game, {}, runners_by_pitch.__setitem__)
            runners = runners_by_pitch.get(seq)
            if runners is None:
                unknown_base_state[side] += 1
                continue
            if not matches_base_state(runners, base_state):
                continue
            ball_four = result == "Walk" and bool(crossed_wild_pitch_limits(x, y))
            if result == "WildPitch" or ball_four:
                advance = wild_pitch_advancement(runners, batter, {"ball_four": ball_four}, points)
                scored = int(advance["advancement"] in {
                    "runner_advanced", "extra_advance_beyond_walk",
                })
        population[side] += 1
        base = bases.get(player)
        modeled = (base, 1.0) if base is not None else None
        if modeled is not None and settings.fatigue:
            appearance = workloads.get((game, player))
            modeled = None
            if appearance and appearance[2] == pitch_count:
                entering, reference, _ = appearance
                load_ratio = (entering + number - 1) / reference
                ramp = min(1.5, max(0.0, (load_ratio - 0.5) / 0.5))
                modeled = (base, 1.0 + 0.25 * ramp * ramp)
        band = (number - 1) // 25 * 25 + 1
        key = (zone, ax, ay, band)
        margin = saved_aim_margin(zone, ax, ay)
        for totals in (by_player[player], by_stratum[key][side], by_aim[zone, margin][side]):
            observe(totals, zone, ax, ay, x, y, result, modeled)
            totals["scored_wild_pitches"] += scored

    if subject not in by_player:
        raise ValueError(
            f"No {pitch_type} pitches for player {subject} in regular season {season}"
            f" with base state {base_state}"
        )

    def detail(totals):
        result = summarize(totals, settings)
        if base_state != "any":
            result["scored_wild_pitches"] = totals["scored_wild_pitches"]
        return result

    aim_usage = [
        {
            "aim_zone": zone, "aim_margin": margin,
            **{
                side: {
                    **detail(rows[side]),
                    "usage_rate": ratio(rows[side]["pitches"], population[side]),
                }
                for side in ("subject", "peers")
            },
        }
        for (zone, margin), rows in sorted(by_aim.items())
    ]
    groups = [
        {
            "aim_zone": zone, "aim_margin": saved_aim_margin(zone, ax, ay),
            "aim_x": ax, "aim_y": ay, "pitch_numbers": [band, band + 24],
            "subject": detail(rows["subject"]),
            "peers": detail(rows["peers"]),
        }
        for (zone, ax, ay, band), rows in sorted(by_stratum.items())
        if rows["subject"]["pitches"]
    ]
    players = []
    for player, totals in sorted(by_player.items(), key=lambda item: (-item[1]["pitches"], item[0])):
        name, control, skill_control = abilities.get(player, (None, None, None))
        players.append({
            "player_id": player, "name": name, "pitcher_control": control,
            "pitch_skill_control": skill_control, "control_components": bases.get(player),
            **detail(totals),
        })
    return {
        "season": season, "pitch_type": pitch_type, "subject_player_id": subject,
        "assumed_settings": vars(settings) if settings else None,
        "base_state": base_state,
        "pitches_excluded_unknown_base_state": {side: unknown_base_state[side] for side in ("subject", "peers")},
        "by_aim_zone_and_margin": aim_usage,
        "matched_by_aim_and_pitch_number": groups, "pitchers": players,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--season", type=int, required=True)
    parser.add_argument("--pitcher-id", type=int, required=True)
    parser.add_argument("--pitch-type", required=True)
    parser.add_argument("--base-state", choices=("any", "no-third", "first-or-second", "third"),
                        default="any", help="Filter on reconstructed pre-pitch running state")
    parser.add_argument("--model", action="store_true", help="Use the current scheduled-game settings as assumptions")
    args = parser.parse_args()
    try:
        settings = Settings.from_env() if args.model else None
        uri = db_from_env().resolve().as_uri() + "?mode=ro"
        with sqlite3.connect(uri, uri=True) as conn:
            conn.execute("BEGIN")
            result = report(conn, args.season, args.pitcher_id, args.pitch_type, settings, args.base_state)
        print(json.dumps(result, indent=2, allow_nan=False))
    except (sqlite3.Error, ValueError) as err:
        parser.exit(1, f"{err}\n")


if __name__ == "__main__":
    main()
