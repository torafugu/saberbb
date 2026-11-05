"""Python reference cases shared with Rust forecast-intent plan tests."""
import argparse
import json
from pathlib import Path

from batting_intent import BattedBallIntent, PlanningInput, frozen_planner
from swing_execution import SwingPlan


def cases():
    planner = frozen_planner()
    result = []
    settings = dict(tilt_deg=15., yaw_batter_deg=0., speed_ratio=1., duration_sec=.18, anchor_fraction=.5)
    inputs = [dict(forecast_position_world_m=[.1 * side, 0., height], forecast_time_sec=.4,
                   attack_angle_deg=10., bat_speed_ms=35., handedness_sign=side,
                   execution_angle_error_deg=2. * side, lever_m=.9, half_length_m=.16)
              for side in (-1., 1.) for height in (.599999, .6, .9, 1.2)]
    extra = [dict(inputs[0], bat_speed_ms=0.), dict(inputs[1], forecast_time_sec=.05)]
    for index, inp in enumerate(inputs + extra):
        for intent in BattedBallIntent:
            built = planner.plan(PlanningInput(**inp), intent)
            reason = None
            try:
                execution = SwingPlan.from_saved(built.forecast_plan).record()
            except ValueError as exc:
                reason = str(exc); execution = None
            result.append(dict(name=f'default_{index}_{intent.value}', input=inp, settings=settings,
                               intent=intent.value, expected_plan=built.record(), expected_execution=execution,
                               execution_invalid_reason=reason))
    for side, ratio, yaw, anchor in [(-1., .6, -20., .35), (1., .8, 20., .65), (1., .9, -10., .5)]:
        inp = dict(inputs[0], handedness_sign=side, execution_angle_error_deg=-4., forecast_position_world_m=[.2, 0., .8])
        options = dict(settings, speed_ratio=ratio, yaw_batter_deg=yaw, anchor_fraction=anchor, duration_sec=.22, tilt_deg=30.)
        built = planner.plan(PlanningInput(**inp), BattedBallIntent.LINE_DRIVE, options['tilt_deg'], yaw, ratio)
        execution = SwingPlan.from_saved(built.forecast_plan, 1., .22, anchor).record()
        result.append(dict(name=f'yaw_{side}_{ratio}', input=inp, settings=options, intent='line_drive',
                           expected_plan=built.record(), expected_execution=execution, execution_invalid_reason=None))
    return dict(version=1, cases=result)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    payload = cases()
    records = ',\n'.join(json.dumps(case, separators=(',', ':'), allow_nan=False) for case in payload['cases'])
    args.output.write_text('{"version":1,"cases":[\n' + records + '\n]}\n')
