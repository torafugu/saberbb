#!/usr/bin/env python3
"""Compare official batting outcomes by exit speed and vertical launch angle."""

import argparse
from collections import Counter
import csv
import json
from pathlib import Path
import sys


OUTCOMES = ("fair_balls", "singles", "home_runs")


def pool_trial(trial):
    """Sum the count in each speed/angle band over backspin and seed."""
    pooled = {outcome: Counter() for outcome in OUTCOMES}
    pa = 0
    for report in trial["reports"]:
        total = report["total"]
        pa += total["pa"]
        try:
            cells = report["fair_launch_diagnostics"]["contact_flight_audit"][
                "by_exit_speed_launch_angle_backspin"
            ]
        except KeyError as error:
            raise ValueError(
                f"seed {report['seed']} lacks the launch breakdown; "
                "run offense_season_check with --vertical-contact-comparison "
                "and --fair-launch-diagnostics together"
            ) from error

        seed_counts = Counter()
        for cell, counts in cells.items():
            speed, angle, _backspin = cell.split("|")
            band = (speed.removeprefix("EV:"), angle.removeprefix("VLA:"))
            for outcome in OUTCOMES:
                pooled[outcome][band] += counts[outcome]
                seed_counts[outcome] += counts[outcome]
        expected = (total["ab"] - total["strikeouts"], total["single"], total["hr"])
        if tuple(seed_counts[outcome] for outcome in OUTCOMES) != expected:
            raise ValueError(f"launch counts do not match official totals for seed {report['seed']}")

    totals = trial["total"]
    pooled_counts = tuple(sum(pooled[outcome].values()) for outcome in OUTCOMES)
    expected = (totals["ab"] - totals["strikeouts"], totals["single"], totals["hr"])
    if pa <= 0 or pa != totals["pa"] or pooled_counts != expected:
        raise ValueError("pooled counts do not match the trial totals")
    return pa, pooled


def compare(report, output):
    reports = report["vertical_contact_trials"]
    trials = {trial["additional_vertical_contact_offset_mm"]: trial for trial in reports}
    if set(trials) != {0, 4} or len(reports) != 2:
        raise ValueError("expected one 0 mm trial and one +4 mm trial")
    baseline_pa, baseline = pool_trial(trials[0])
    shifted_pa, shifted = pool_trial(trials[4])

    rows = []
    for band in baseline["fair_balls"].keys() | shifted["fair_balls"].keys():
        row = {"exit_speed_ms": band[0], "launch_angle_deg": band[1]}
        for outcome in OUTCOMES:
            before = baseline[outcome][band]
            after = shifted[outcome][band]
            before_rate = 100 * before / baseline_pa
            after_rate = 100 * after / shifted_pa
            row.update({
                f"{outcome}_0mm": before,
                f"{outcome}_4mm": after,
                f"{outcome}_per_100_pa_0mm": before_rate,
                f"{outcome}_per_100_pa_4mm": after_rate,
                f"delta_{outcome}_per_100_pa": after_rate - before_rate,
            })
        rows.append(row)

    columns = ["exit_speed_ms", "launch_angle_deg"]
    for outcome in OUTCOMES:
        columns.extend((
            f"{outcome}_0mm", f"{outcome}_4mm",
            f"{outcome}_per_100_pa_0mm", f"{outcome}_per_100_pa_4mm",
            f"delta_{outcome}_per_100_pa",
        ))
    writer = csv.DictWriter(output, fieldnames=columns, lineterminator="\n")
    writer.writeheader()
    writer.writerows(sorted(rows, key=lambda row: (
        -row["delta_singles_per_100_pa"], row["exit_speed_ms"], row["launch_angle_deg"]
    )))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path, help="JSON from the combined full-game diagnostic")
    args = parser.parse_args()
    try:
        compare(json.loads(args.report.read_bytes()), sys.stdout)
    except (OSError, KeyError, ValueError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
