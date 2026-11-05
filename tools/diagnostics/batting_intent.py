"""Explicit batting intent -> forecast-only aim -> existing diagnostic swing plan."""
from dataclasses import dataclass
from enum import Enum
import json
import math
from pathlib import Path

from planned_axis import plan_axis_swing, with_planned_speed
from pre_swing_aim import AimPolicy, Forecast, finite, read_policies

FROZEN_POLICIES = Path(__file__).with_name('pre_swing_aim_seed2_frozen.json')


class BattedBallIntent(str, Enum):
    CONTACT_FIRST = 'contact_first'
    LINE_DRIVE = 'line_drive'


POLICY_NAMES = {
    BattedBallIntent.CONTACT_FIRST: 'contact_center',
    BattedBallIntent.LINE_DRIVE: 'line_drive_low_center',
}


@dataclass(frozen=True)
class PlanningInput:
    """Forecast, batter configuration and existing angular execution error only."""
    forecast_position_world_m: tuple
    forecast_time_sec: float
    attack_angle_deg: float
    bat_speed_ms: float
    handedness_sign: float
    execution_angle_error_deg: float
    lever_m: float = .9
    half_length_m: float = .16

    def __post_init__(self):
        position = self.forecast_position_world_m
        if not isinstance(position, (tuple, list)) or len(position) != 3:
            raise ValueError('invalid_intent_forecast_position')
        for x in (*position, self.forecast_time_sec, self.attack_angle_deg, self.bat_speed_ms,
                  self.handedness_sign, self.execution_angle_error_deg, self.lever_m, self.half_length_m):
            finite(x)
        if (self.forecast_time_sec <= 0 or abs(self.attack_angle_deg) > 90 or self.bat_speed_ms < 0
                or self.handedness_sign not in (-1, 1) or not 0 < self.half_length_m < self.lever_m):
            raise ValueError('invalid_intent_planning_input')
        object.__setattr__(self, 'forecast_position_world_m', tuple(position))

    @classmethod
    def from_forecast_plan(cls, saved):
        """Whitelist pre-swing inputs; do not reuse saved aim, actual point or contact."""
        velocity = saved['ideal_velocity_world_ms']
        if not isinstance(velocity, (tuple, list)) or len(velocity) != 3:
            raise ValueError('invalid_intent_planned_velocity')
        for value in velocity:
            finite(value)
        if abs(velocity[0]) > 1e-8:
            raise ValueError('invalid_intent_planned_velocity')
        axis_x = finite(saved['ideal_axis_world'][0])
        if abs(axis_x) < 1e-8:
            raise ValueError('ambiguous_intent_handedness')
        return cls(tuple(saved['forecast_position_world_m']), saved['nominal_time_sec'],
                   math.degrees(math.atan2(velocity[2], velocity[1])), math.hypot(*velocity),
                   1 if axis_x > 0 else -1, saved['execution_angle_error_deg'],
                   saved['lever_m'], saved['half_length_m'])


@dataclass(frozen=True)
class IntendedSwing:
    intent: BattedBallIntent
    aim_policy: str
    aim_z_offset_m: float
    forecast_plan: dict

    def record(self):
        return dict(intent=self.intent.value, aim_policy=self.aim_policy,
                    aim_z_offset_m=self.aim_z_offset_m, forecast_plan=self.forecast_plan)


class IntentPlanner:
    def __init__(self, config):
        by_name = {p.name: p for p in read_policies(config)}
        if not set(POLICY_NAMES.values()) <= set(by_name):
            raise ValueError('missing_declared_intent_policy')
        self.policies = {intent: by_name[name] for intent, name in POLICY_NAMES.items()}

    def diagnostic_policies(self, intents):
        """Label diagnostics by explicitly supplied intent while retaining fixed rules."""
        return [AimPolicy(intent.value, self.policies[intent].intent,
                          self.policies[intent].height_edges_m, self.policies[intent].aim_z_mm)
                for intent in intents]

    def plan(self, inputs, intent, tilt_deg=15., yaw_batter_deg=0., speed_ratio=1.):
        if not isinstance(inputs, PlanningInput) or not isinstance(intent, BattedBallIntent):
            raise ValueError('explicit_intent_and_planning_input_required')
        if not 0 <= finite(tilt_deg) <= 60:
            raise ValueError('invalid_intent_tilt')
        finite(yaw_batter_deg)
        chosen = self.policies[intent]
        aim = chosen.choose(Forecast(inputs.forecast_position_world_m[2])) / 1000
        prediction = dict(forecast=dict(position_world_m=inputs.forecast_position_world_m,
                                       time_sec=inputs.forecast_time_sec),
            handedness_sign=inputs.handedness_sign, planned_bat_angle_deg=tilt_deg,
            planned_attack_angle_deg=inputs.attack_angle_deg, bat_speed_ms=inputs.bat_speed_ms,
            execution_angle_error_deg=inputs.execution_angle_error_deg, residual_execution_delay_sec=0.)
        saved = plan_axis_swing(prediction, inputs.lever_m, inputs.half_length_m, yaw_batter_deg,
                               (0., 0., aim))
        return IntendedSwing(intent, chosen.name, aim, with_planned_speed(saved, speed_ratio))


def frozen_planner():
    return IntentPlanner(json.loads(FROZEN_POLICIES.read_text()))
