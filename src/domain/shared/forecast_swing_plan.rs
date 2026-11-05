//! Diagnostic forecast-only intent plans. No contact truth, RNG, or live outcome changes.
use crate::domain::shared::swing_prediction::{PREDICTION_SOURCE, SwingPredictionInput};
use crate::domain::strategy::batting_strategy::BattedBallIntent;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

pub const INTENT_PLAN_SOURCE: &str = "forecast_intent_shadow_v1";
type V3 = [f64; 3];

pub(crate) fn add(a: V3, b: V3) -> V3 { std::array::from_fn(|i| a[i] + b[i]) }
pub(crate) fn sub(a: V3, b: V3) -> V3 { std::array::from_fn(|i| a[i] - b[i]) }
pub(crate) fn mul(a: V3, s: f64) -> V3 { a.map(|x| x * s) }
pub(crate) fn dot(a: V3, b: V3) -> f64 { a.iter().zip(b).map(|(x, y)| x * y).sum() }
pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [a[1]*b[2] - a[2]*b[1], a[2]*b[0] - a[0]*b[2], a[0]*b[1] - a[1]*b[0]]
}
pub(crate) fn rotate(a: V3, omega: V3, t: f64) -> V3 {
    let speed = dot(omega, omega).sqrt();
    if speed == 0.0 { return a; }
    let k = mul(omega, 1.0 / speed);
    let phase = speed * t;
    add(add(mul(a, phase.cos()), mul(cross(k, a), phase.sin())),
        mul(k, dot(k, a) * (1.0 - phase.cos())))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanningInput {
    pub forecast_position_world_m: V3,
    pub forecast_time_sec: f64,
    pub attack_angle_deg: f64,
    pub bat_speed_ms: f64,
    pub handedness_sign: f64,
    pub execution_angle_error_deg: f64,
    pub lever_m: f64,
    pub half_length_m: f64,
}

impl From<&SwingPredictionInput> for PlanningInput {
    fn from(p: &SwingPredictionInput) -> Self {
        Self { forecast_position_world_m: p.forecast.position_world_m,
            forecast_time_sec: p.forecast.time_sec, attack_angle_deg: p.planned_attack_angle_deg,
            bat_speed_ms: p.bat_speed_ms, handedness_sign: p.handedness_sign,
            execution_angle_error_deg: p.execution_angle_error_deg, lever_m: 0.9, half_length_m: 0.16 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PlanSettings {
    pub tilt_deg: f64,
    pub yaw_batter_deg: f64,
    pub speed_ratio: f64,
    pub duration_sec: f64,
    pub anchor_fraction: f64,
}

impl Default for PlanSettings {
    fn default() -> Self {
        Self { tilt_deg: 15.0, yaw_batter_deg: 0.0, speed_ratio: 1.0,
            duration_sec: 0.18, anchor_fraction: 0.5 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForecastSwingPlan {
    pub plan_version: u8,
    pub source: String,
    pub lever_m: f64,
    pub half_length_m: f64,
    pub nominal_time_sec: f64,
    pub forecast_position_world_m: V3,
    pub pivot_world_m: V3,
    pub pivot_velocity_world_ms: V3,
    pub ideal_axis_world: V3,
    pub actual_axis_world: V3,
    pub ideal_point_world_m: V3,
    pub actual_point_world_m: V3,
    pub ideal_velocity_world_ms: V3,
    pub actual_velocity_world_ms: V3,
    pub ideal_omega_world_rad_s: V3,
    pub actual_omega_world_rad_s: V3,
    pub execution_rotation_world_rad: V3,
    pub execution_angle_error_deg: f64,
    pub planned_aim_offset_world_m: V3,
    pub planned_aim_point_world_m: V3,
    pub planned_speed_ratio: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntendedSwing {
    pub intent: BattedBallIntent,
    pub aim_policy: String,
    pub aim_z_offset_m: f64,
    pub forecast_plan: ForecastSwingPlan,
}

/// Same contract as Python IntentPlanner.plan; no actual ball/contact argument.
pub fn plan_intended_swing(p: &PlanningInput, intent: BattedBallIntent, s: PlanSettings)
    -> Result<IntendedSwing, &'static str>
{
    let values = [p.forecast_time_sec, p.attack_angle_deg, p.bat_speed_ms, p.handedness_sign,
        p.execution_angle_error_deg, p.lever_m, p.half_length_m, s.tilt_deg, s.yaw_batter_deg,
        s.speed_ratio, s.duration_sec, s.anchor_fraction];
    if !values.iter().chain(p.forecast_position_world_m.iter()).all(|x| x.is_finite())
        || p.forecast_time_sec <= 0.0 || p.attack_angle_deg.abs() > 90.0 || p.bat_speed_ms < 0.0
        || ![-1.0, 1.0].contains(&p.handedness_sign) || p.half_length_m <= 0.0 || p.lever_m <= p.half_length_m
        || !(0.0..=60.0).contains(&s.tilt_deg) || s.yaw_batter_deg.abs() > 45.0
        || s.speed_ratio <= 0.0 || s.speed_ratio > 1.0 || s.duration_sec <= 0.0
        || s.anchor_fraction <= 0.0 || s.anchor_fraction >= 1.0
    { return Err("invalid_intent_planning_input_or_settings"); }
    let aim_z = intent.aim_z_offset_m(p.forecast_position_world_m[2])?;
    let offset = [0.0, 0.0, aim_z];
    let theta = s.tilt_deg.to_radians();
    let attack = p.attack_angle_deg.to_radians();
    let h = p.handedness_sign;
    let yaw = [0.0, 0.0, h * s.yaw_batter_deg.to_radians()];
    let axis = rotate([h * theta.cos(), 0.0, theta.sin()], yaw, 1.0);
    let velocity = [0.0, p.bat_speed_ms * attack.cos(), p.bat_speed_ms * attack.sin()];
    let pivot = sub(p.forecast_position_world_m, mul(axis, p.lever_m));
    let vp = mul(axis, dot(velocity, axis));
    let omega = mul(cross(axis, sub(velocity, vp)), 1.0 / p.lever_m);
    let error = rotate([0.0, -h * p.execution_angle_error_deg.to_radians(), 0.0], yaw, 1.0);
    let actual_axis = rotate(axis, error, 1.0);
    let actual_omega = rotate(omega, error, 1.0);
    let actual_point = add(pivot, mul(actual_axis, p.lever_m));
    let actual_velocity = add(vp, cross(actual_omega, mul(actual_axis, p.lever_m)));
    let plan = ForecastSwingPlan { plan_version: 1, source: PREDICTION_SOURCE.into(),
        lever_m: p.lever_m, half_length_m: p.half_length_m, nominal_time_sec: p.forecast_time_sec,
        forecast_position_world_m: p.forecast_position_world_m, pivot_world_m: add(pivot, offset),
        pivot_velocity_world_ms: mul(vp, s.speed_ratio), ideal_axis_world: axis, actual_axis_world: actual_axis,
        ideal_point_world_m: add(p.forecast_position_world_m, offset), actual_point_world_m: add(actual_point, offset),
        ideal_velocity_world_ms: mul(velocity, s.speed_ratio), actual_velocity_world_ms: mul(actual_velocity, s.speed_ratio),
        ideal_omega_world_rad_s: mul(omega, s.speed_ratio), actual_omega_world_rad_s: mul(actual_omega, s.speed_ratio),
        execution_rotation_world_rad: error, execution_angle_error_deg: p.execution_angle_error_deg,
        planned_aim_offset_world_m: offset, planned_aim_point_world_m: add(p.forecast_position_world_m, offset),
        planned_speed_ratio: s.speed_ratio };
    let vectors = [plan.pivot_world_m, plan.pivot_velocity_world_ms, plan.ideal_axis_world,
        plan.actual_axis_world, plan.ideal_point_world_m, plan.actual_point_world_m, plan.ideal_velocity_world_ms,
        plan.actual_velocity_world_ms, plan.ideal_omega_world_rad_s, plan.actual_omega_world_rad_s,
        plan.execution_rotation_world_rad];
    if !vectors.iter().flatten().all(|x| x.is_finite()) { return Err("non_finite_intent_plan"); }
    Ok(IntendedSwing { intent, aim_policy: intent.aim_policy().into(), aim_z_offset_m: aim_z, forecast_plan: plan })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionSwingPlan {
    pub nominal_time_sec: f64,
    pub start_time_sec: f64,
    pub planned_effort: f64,
    pub duration_sec: f64,
    pub anchor_fraction: f64,
    pub lever_m: f64,
    pub half_length_m: f64,
    pub forecast_position_world_m: V3,
    pub reference_point_world_m: V3,
    pub reference_axis_world: V3,
    pub start_pivot_world_m: V3,
    pub start_axis_world: V3,
    pub peak_pivot_velocity_world_ms: V3,
    pub peak_omega_world_rad_s: V3,
    pub version: u8,
    pub source: String,
    pub start_point_world_m: V3,
}

pub fn execution_plan(p: &ForecastSwingPlan, s: PlanSettings) -> Result<ExecutionSwingPlan, &'static str> {
    if ![s.duration_sec, s.anchor_fraction, p.nominal_time_sec].iter().all(|x| x.is_finite())
        || s.duration_sec <= 0.0 || s.anchor_fraction <= 0.0 || s.anchor_fraction >= 1.0 || p.nominal_time_sec <= 0.0
    { return Err("invalid_swing_execution"); }
    let start_time = p.nominal_time_sec - s.anchor_fraction * s.duration_sec;
    if start_time < 0.0 { return Err("execution_starts_before_release"); }
    let progress = s.duration_sec * (s.anchor_fraction / 2.0 - (2.0 * PI * s.anchor_fraction).sin() / (4.0 * PI));
    let start_axis = rotate(p.actual_axis_world, p.actual_omega_world_rad_s, -progress);
    let start_pivot = sub(p.pivot_world_m, mul(p.pivot_velocity_world_ms, progress));
    let start_point = add(start_pivot, mul(start_axis, p.lever_m));
    if !start_axis.iter().chain(start_pivot.iter()).chain(start_point.iter()).all(|x| x.is_finite()) {
        return Err("non_finite_execution_plan");
    }
    Ok(ExecutionSwingPlan { nominal_time_sec: p.nominal_time_sec, start_time_sec: start_time,
        planned_effort: 1.0, duration_sec: s.duration_sec, anchor_fraction: s.anchor_fraction,
        lever_m: p.lever_m, half_length_m: p.half_length_m, forecast_position_world_m: p.forecast_position_world_m,
        reference_point_world_m: p.actual_point_world_m, reference_axis_world: p.actual_axis_world,
        start_pivot_world_m: start_pivot, start_axis_world: start_axis, peak_pivot_velocity_world_ms: p.pivot_velocity_world_ms,
        peak_omega_world_rad_s: p.actual_omega_world_rad_s, version: 1,
        source: "forecast_target_and_planned_effort_v1".into(), start_point_world_m: start_point })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiagnosticIntentPlan {
    pub intent: BattedBallIntent,
    pub invalid_reason: Option<String>,
    pub plan: Option<IntendedSwing>,
    pub execution_plan: Option<ExecutionSwingPlan>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntentPlanDiagnostics {
    pub version: u8,
    pub source: String,
    pub input: PlanningInput,
    pub settings: PlanSettings,
    pub available_time_sec: f64,
    pub plans: Vec<DiagnosticIntentPlan>,
}

/// Shadow alternatives only: no choice is fed back into the live batting resolver.
pub fn diagnostic_intent_plans(prediction: &SwingPredictionInput) -> IntentPlanDiagnostics {
    let input = PlanningInput::from(prediction);
    let settings = PlanSettings::default();
    let available = prediction.observation.time_sec;
    let plans = [BattedBallIntent::ContactFirst, BattedBallIntent::LineDrive].into_iter().map(|intent| {
        let mut record = DiagnosticIntentPlan { intent, invalid_reason: None, plan: None, execution_plan: None };
        let planned = plan_intended_swing(&input, intent, settings);
        match planned {
            Err(reason) => record.invalid_reason = Some(reason.into()),
            Ok(plan) => {
                let result = if prediction.residual_execution_delay_sec != 0.0 {
                    Err("unsupported_residual_execution_delay")
                } else if !available.is_finite() || available < 0.0 || available >= input.forecast_time_sec {
                    Err("invalid_plan_available_clock")
                } else {
                    execution_plan(&plan.forecast_plan, settings).and_then(|execution| {
                        if execution.start_time_sec < available - 1e-10 { Err("execution_starts_before_plan") }
                        else { Ok(execution) }
                    })
                };
                match result {
                    Ok(execution) => record.execution_plan = Some(execution),
                    Err(reason) => record.invalid_reason = Some(reason.into()),
                }
                record.plan = Some(plan);
            }
        }
        record
    }).collect();
    IntentPlanDiagnostics { version: 1, source: INTENT_PLAN_SOURCE.into(), input, settings, available_time_sec: available, plans }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn same_json(expected: &Value, actual: &Value, path: &str) {
        match (expected, actual) {
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(a.len(), b.len(), "keys at {path}");
                for (key, value) in a { same_json(value, &b[key], &format!("{path}.{key}")); }
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len(), "array at {path}");
                for (i, (x, y)) in a.iter().zip(b).enumerate() { same_json(x, y, &format!("{path}[{i}]")); }
            }
            (Value::Number(a), Value::Number(b)) => {
                assert!((a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-9,
                    "number at {path}: {a} != {b}");
            }
            _ => assert_eq!(expected, actual, "value at {path}"),
        }
    }

    #[test]
    fn rust_plans_and_pulse_start_poses_match_python_contract() {
        let fixture: Value = serde_json::from_str(include_str!("../../../tests/fixtures/intent_plan_contract.json")).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let input: PlanningInput = serde_json::from_value(case["input"].clone()).unwrap();
            let settings: PlanSettings = serde_json::from_value(case["settings"].clone()).unwrap();
            let intent: BattedBallIntent = serde_json::from_value(case["intent"].clone()).unwrap();
            let plan = plan_intended_swing(&input, intent, settings).unwrap();
            let name = case["name"].as_str().unwrap();
            same_json(&case["expected_plan"], &serde_json::to_value(&plan).unwrap(), name);
            match execution_plan(&plan.forecast_plan, settings) {
                Ok(execution) => {
                    assert!(case["execution_invalid_reason"].is_null());
                    same_json(&case["expected_execution"], &serde_json::to_value(execution).unwrap(), name);
                }
                Err(reason) => assert_eq!(case["execution_invalid_reason"].as_str(), Some(reason), "{name}"),
            }
        }
    }

    #[test]
    fn policy_offsets_match_the_frozen_policy_file_and_reject_nonfinite_height() {
        let config: Value = serde_json::from_str(include_str!("../../../tools/diagnostics/pre_swing_aim_seed2_frozen.json")).unwrap();
        for height in [0.3, 0.599999, 0.6, 0.899999, 0.9, 1.3] {
            let band = if height < 0.6 { 0 } else if height < 0.9 { 1 } else { 2 };
            for intent in [BattedBallIntent::ContactFirst, BattedBallIntent::LineDrive] {
                let expected = config["policies"][intent.aim_policy()]["aim_z_mm"][band].as_f64().unwrap() / 1000.0;
                assert_eq!(intent.aim_z_offset_m(height).unwrap(), expected);
            }
        }
        assert!(BattedBallIntent::LineDrive.aim_z_offset_m(f64::NAN).is_err());
    }

    #[test]
    fn invalid_parameters_and_clock_are_explicit() {
        let input = PlanningInput { forecast_position_world_m: [0.1, 0.0, 0.8], forecast_time_sec: 0.4,
            attack_angle_deg: 10.0, bat_speed_ms: 35.0, handedness_sign: 1.0,
            execution_angle_error_deg: 0.0, lever_m: 0.9, half_length_m: 0.16 };
        let mut bad = input.clone(); bad.forecast_time_sec = f64::INFINITY;
        assert!(plan_intended_swing(&bad, BattedBallIntent::LineDrive, PlanSettings::default()).is_err());
        bad = input.clone(); bad.handedness_sign = 0.0;
        assert!(plan_intended_swing(&bad, BattedBallIntent::LineDrive, PlanSettings::default()).is_err());
        let bad_settings = PlanSettings { speed_ratio: 0.0, ..PlanSettings::default() };
        assert!(plan_intended_swing(&input, BattedBallIntent::LineDrive, bad_settings).is_err());
    }
}
