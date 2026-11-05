"""Python reference trajectory samples and conservative finite-segment searches."""
import argparse
import json
import math
from pathlib import Path

from batting_intent import BattedBallIntent, PlanningInput, frozen_planner
from rotating_bat_probe import search, add, mul
from swing_execution import SwingPlan, PlanExecutionMotion, ExecutionDeviation

SETTINGS = dict(window_start_sec=-.06, window_end_sec=.06, effective_radius_m=.07, tolerance_m=1e-6, max_iterations=4000)
GEOMETRY_FIELDS = ('clearance_m', 'axial_m', 'closest_axial_m', 'normal', 'bat_velocity', 'axis', 'contact_region')


def motion_for(truth, execution):
    state = dict(input=dict(truth, timing_offset_sec=0.), bat_ideal_position_world_m=execution.reference_point_world_m,
                 bat_axis_world=execution.reference_axis_world, bat_velocity_world_ms=(0., 0., 0.))
    return PlanExecutionMotion(state, execution, ExecutionDeviation())


def pose_samples(truth, execution):
    motion = motion_for(truth, execution)
    samples = []
    for phase in (-.03, 0., .03):
        point, axis = motion.pose(phase, 0.)
        samples.append(dict(phase_sec=phase, point_world_m=point, axis_world=axis,
                            velocity_world_ms=motion.velocity_at_phase(phase, 0.),
                            angular_velocity_world_rad_s=motion.angular_velocity(phase, 0.)))
    return samples


def evaluate(truth, execution, settings=SETTINGS):
    samples = pose_samples(truth, execution)
    values = [truth['reference_time_sec_since_release'], *truth['ball_position_world_m'], *truth['ball_velocity_world_ms'],
              settings['window_start_sec'], settings['window_end_sec'], settings['effective_radius_m'], settings['tolerance_m']]
    if (not all(math.isfinite(x) for x in values) or settings['window_start_sec'] >= settings['window_end_sec']
        or settings['effective_radius_m'] <= 0 or settings['tolerance_m'] <= 0 or settings['max_iterations'] <= 0
        or truth['reference_time_sec_since_release'] + settings['window_start_sec'] < 0):
        raise ValueError('invalid_contact_search_input')
    motion = motion_for(truth, execution)
    motion.radius = settings['effective_radius_m']
    delay = execution.nominal_time_sec - truth['reference_time_sec_since_release']
    result = search(motion, delay, settings['window_start_sec'], settings['window_end_sec'],
                    settings['tolerance_m'], settings['max_iterations'])
    contact = result['status'] == 'contact'
    normalized = dict(status=result['status'], time_offset_sec=result['time_offset_sec'] if contact else None,
                      iterations=result['iterations'], min_evaluated_clearance_m=result['min_evaluated_clearance_m'],
                      geometry={k: result[k] for k in GEOMETRY_FIELDS} if contact else None)
    return samples, normalized


def cases():
    planner = frozen_planner(); rows = []
    for side, error, ratio in [(1., 0., 1.), (-1., 2., .8)]:
        for intent in BattedBallIntent:
            p = PlanningInput((.1, 0., .8), .4, 10., 35., side, error)
            planned = planner.plan(p, intent, speed_ratio=ratio)
            execution = SwingPlan.from_saved(planned.forecast_plan)
            base = dict(reference_time_sec_since_release=.4, ball_position_world_m=[.1, 0., .8],
                ball_velocity_world_ms=[0., -40., -2.], bat_speed_ms=35., bat_angle_deg=15., attack_angle_deg=10.,
                handedness_sign=side, aim_error_world_m=[0.,0.,0.], timing_offset_sec=0., legacy_timing_impact_x_m=0.,
                legacy_contact_offset_xz_m=[0.,0.], legacy_timing_position_scale=None)
            for kind in ('side', 'miss', 'cap', 'overlap', 'unresolved'):
                truth = dict(base); settings = dict(SETTINGS)
                if kind == 'miss': truth['ball_position_world_m'] = [3., 0., 2.]
                if kind == 'cap':
                    truth['ball_position_world_m'] = add(execution.reference_point_world_m, mul(execution.reference_axis_world, .20))
                if kind == 'overlap':
                    motion = motion_for(truth, execution)
                    point, _ = motion.pose(-.06, 0.)
                    truth['ball_position_world_m'] = add(point, mul(truth['ball_velocity_world_ms'], .06))
                if kind == 'unresolved': settings['max_iterations'] = 1
                samples, result = evaluate(truth, execution, settings)
                rows.append(dict(name=f'{side}_{intent.value}_{kind}', execution_plan=execution.record(), truth=truth,
                                 settings=settings, samples=samples, search=result))
    return dict(version=1, cases=rows)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    records = ',\n'.join(json.dumps(c, separators=(',', ':'), allow_nan=False) for c in cases()['cases'])
    args.output.write_text('{"version":1,"cases":[\n' + records + '\n]}\n')
