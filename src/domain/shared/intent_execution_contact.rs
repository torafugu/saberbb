//! Forward execution and finite-segment geometry only; no live batting/collision changes.
use super::forecast_swing_plan::{ExecutionSwingPlan, add, sub, mul, dot, cross, rotate};
use super::contact_state::{ContactStateInput, ContactStateRecord};
use crate::domain::strategy::batting_strategy::{BattedBallIntent, BattingIntentDecision};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ContactSearchSettings {
    pub window_start_sec: f64,
    pub window_end_sec: f64,
    pub effective_radius_m: f64,
    pub tolerance_m: f64,
    pub max_iterations: u32,
}
impl Default for ContactSearchSettings {
    fn default() -> Self {
        Self { window_start_sec: -0.06, window_end_sec: 0.06,
            effective_radius_m: 0.07, tolerance_m: 1e-6, max_iterations: 4000 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PoseSample {
    pub phase_sec: f64,
    pub point_world_m: [f64; 3],
    pub axis_world: [f64; 3],
    pub velocity_world_ms: [f64; 3],
    pub angular_velocity_world_rad_s: [f64; 3],
}

/// Phase is absolute time minus forecast nominal time, never anchored to the true ball.
pub fn execution_pose(plan: &ExecutionSwingPlan, phase_sec: f64) -> PoseSample {
    let u = ((phase_sec + plan.anchor_fraction * plan.duration_sec) / plan.duration_sec).clamp(0.0, 1.0);
    let progress = plan.duration_sec * (u/2.0 - (2.0*PI*u).sin()/(4.0*PI));
    let gain = if u > 0.0 && u < 1.0 { (PI*u).sin().powi(2) } else { 0.0 };
    let axis = rotate(plan.start_axis_world, plan.peak_omega_world_rad_s, progress);
    let pivot = add(plan.start_pivot_world_m, mul(plan.peak_pivot_velocity_world_ms, progress));
    let omega = mul(plan.peak_omega_world_rad_s, gain);
    let velocity = add(mul(plan.peak_pivot_velocity_world_ms, gain), cross(omega, mul(axis, plan.lever_m)));
    PoseSample { phase_sec, point_world_m: add(pivot, mul(axis, plan.lever_m)),
        axis_world: axis, velocity_world_ms: velocity, angular_velocity_world_rad_s: omega }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeometrySample {
    pub clearance_m: f64,
    pub axial_m: f64,
    pub closest_axial_m: f64,
    pub normal: Option<[f64; 3]>,
    pub bat_velocity: [f64; 3],
    pub axis: [f64; 3],
    pub contact_region: String,
}

/// Actual ball state is allowed ONLY in this verification layer, not in plan generation.
pub fn geometry_at(plan: &ExecutionSwingPlan, truth: &ContactStateInput, t: f64, radius: f64) -> GeometrySample {
    let delay = plan.nominal_time_sec - truth.reference_time_sec_since_release;
    let pose = execution_pose(plan, t - delay);
    let ball = add(truth.ball_position_world_m, mul(truth.ball_velocity_world_ms, t));
    let displacement = sub(ball, pose.point_world_m);
    let axial = dot(displacement, pose.axis_world);
    let clamped = axial.clamp(-plan.half_length_m, plan.half_length_m);
    let radial = sub(ball, add(pose.point_world_m, mul(pose.axis_world, clamped)));
    let distance = dot(radial, radial).sqrt();
    let velocity = add(pose.velocity_world_ms, cross(pose.angular_velocity_world_rad_s, mul(pose.axis_world, clamped)));
    GeometrySample { clearance_m: distance - radius, axial_m: axial, closest_axial_m: clamped,
        normal: if distance > 0.0 { Some(mul(radial, 1.0/distance)) } else { None },
        bat_velocity: velocity, axis: pose.axis_world,
        contact_region: if axial.abs() <= plan.half_length_m { "side" } else { "cap" }.into() }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContactSearchResult {
    pub status: String,
    pub time_offset_sec: Option<f64>,
    pub iterations: u32,
    pub min_evaluated_clearance_m: f64,
    pub geometry: Option<GeometrySample>,
}

pub fn search_contact(plan: &ExecutionSwingPlan, truth: &ContactStateInput, s: ContactSearchSettings)
    -> Result<ContactSearchResult, &'static str>
{
    if ![plan.nominal_time_sec, plan.duration_sec, plan.anchor_fraction, plan.lever_m, plan.half_length_m]
        .iter().chain(plan.start_axis_world.iter()).chain(plan.start_pivot_world_m.iter())
        .chain(plan.peak_pivot_velocity_world_ms.iter()).chain(plan.peak_omega_world_rad_s.iter())
        .all(|x| x.is_finite()) || plan.duration_sec <= 0.0 || plan.lever_m < 0.0
        || plan.half_length_m <= 0.0 || !(0.0..=1.0).contains(&plan.anchor_fraction)
    { return Err("invalid_contact_execution_plan"); }
    if ![s.window_start_sec, s.window_end_sec, s.effective_radius_m, s.tolerance_m,
        truth.reference_time_sec_since_release].iter().chain(truth.ball_position_world_m.iter())
        .chain(truth.ball_velocity_world_ms.iter()).all(|x| x.is_finite())
        || s.window_start_sec >= s.window_end_sec || s.effective_radius_m <= 0.0 || s.tolerance_m <= 0.0
        || s.max_iterations == 0 || truth.reference_time_sec_since_release + s.window_start_sec < 0.0
    { return Err("invalid_contact_search_input"); }
    let speed_bound = dot(truth.ball_velocity_world_ms, truth.ball_velocity_world_ms).sqrt()
        + dot(plan.peak_pivot_velocity_world_ms, plan.peak_pivot_velocity_world_ms).sqrt()
        + dot(plan.peak_omega_world_rad_s, plan.peak_omega_world_rad_s).sqrt() * (plan.lever_m + plan.half_length_m);
    if !speed_bound.is_finite() { return Err("non_finite_contact_speed_bound"); }
    let mut t = s.window_start_sec;
    let mut minimum = f64::INFINITY;
    let mut iterations = 0;
    let mut status = "unresolved";
    let mut contact = None;
    for iteration in 1..=s.max_iterations {
        iterations = iteration;
        let value = geometry_at(plan, truth, t, s.effective_radius_m);
        if !value.clearance_m.is_finite() { return Err("non_finite_contact_geometry"); }
        let gap = value.clearance_m;
        minimum = minimum.min(gap);
        if gap <= s.tolerance_m {
            if iteration == 1 { status = "overlap_at_start"; }
            else { status = "contact"; contact = Some(value); }
            break;
        }
        if t >= s.window_end_sec || speed_bound == 0.0 { status = "no_contact"; break; }
        let next = s.window_end_sec.min(t + 0.9 * gap / speed_bound);
        if next <= t { break; }
        t = next;
    }
    Ok(ContactSearchResult { status: status.into(), time_offset_sec: if status == "contact" { Some(t) } else { None },
        iterations, min_evaluated_clearance_m: minimum, geometry: contact })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntentContactProbe {
    pub intent: BattedBallIntent,
    pub selected: bool,
    pub invalid_reason: Option<String>,
    pub samples: Vec<PoseSample>,
    pub search: Option<ContactSearchResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionContactDiagnostics {
    pub version: u8,
    pub source: String,
    pub decision: BattingIntentDecision,
    pub settings: ContactSearchSettings,
    pub probes: Vec<IntentContactProbe>,
}

pub fn attach_execution_contacts(record: &mut ContactStateRecord) {
    let Some(state) = record.state.as_mut() else { return; };
    let Some(observed) = state.observed_intent_plans.as_ref() else { return; };
    let Some(plans) = observed.intent_plans.as_ref() else { return; };
    let settings = ContactSearchSettings::default();
    let probes = plans.plans.iter().map(|p| {
        let mut result = IntentContactProbe { intent: p.intent, selected: p.intent == observed.decision.selected_intent,
            invalid_reason: p.invalid_reason.clone(), samples: Vec::new(), search: None };
        if result.invalid_reason.is_some() { return result; }
        if let Some(execution) = p.execution_plan.as_ref() {
            result.samples = [-0.03, 0.0, 0.03].into_iter().map(|q| execution_pose(execution, q)).collect();
            match search_contact(execution, &state.input, settings) {
                Ok(search) => result.search = Some(search),
                Err(reason) => result.invalid_reason = Some(reason.into()),
            }
        } else { result.invalid_reason = Some("missing_execution_plan".into()); }
        result
    }).collect();
    state.execution_contact = Some(ExecutionContactDiagnostics { version: 1,
        source: "intent_execution_contact_shadow_v1".into(), decision: observed.decision, settings, probes });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn same(a: &Value, b: &Value) {
        match (a,b) {
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(a.len(), b.len());
                for (k,v) in a { same(v, &b[k]); }
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len());
                for (x,y) in a.iter().zip(b) { same(x,y); }
            }
            (Value::Number(a), Value::Number(b)) => assert!((a.as_f64().unwrap()-b.as_f64().unwrap()).abs() < 1e-8, "{a} != {b}"),
            _ => assert_eq!(a,b),
        }
    }

    #[test]
    fn forward_motion_and_contact_search_match_python_reference() {
        let fixture: Value = serde_json::from_str(include_str!("../../../tests/fixtures/intent_execution_contract.json")).unwrap();
        let mut statuses = std::collections::BTreeSet::new();
        for case in fixture["cases"].as_array().unwrap() {
            let plan: ExecutionSwingPlan = serde_json::from_value(case["execution_plan"].clone()).unwrap();
            let truth: ContactStateInput = serde_json::from_value(case["truth"].clone()).unwrap();
            let settings: ContactSearchSettings = serde_json::from_value(case["settings"].clone()).unwrap();
            let samples: Vec<_> = [-0.03,0.0,0.03].into_iter().map(|q| execution_pose(&plan,q)).collect();
            same(&case["samples"], &serde_json::to_value(samples).unwrap());
            let result = search_contact(&plan, &truth, settings).unwrap();
            same(&case["search"], &serde_json::to_value(&result).unwrap());
            statuses.insert(result.status);
        }
        for expected in ["contact", "no_contact", "overlap_at_start", "unresolved"] {
            assert!(statuses.contains(expected));
        }
    }

    #[test]
    fn malformed_plan_and_search_settings_are_explicit_errors() {
        let fixture: Value = serde_json::from_str(include_str!("../../../tests/fixtures/intent_execution_contract.json")).unwrap();
        let case = &fixture["cases"][0];
        let mut plan: ExecutionSwingPlan = serde_json::from_value(case["execution_plan"].clone()).unwrap();
        let truth: ContactStateInput = serde_json::from_value(case["truth"].clone()).unwrap();
        let settings = ContactSearchSettings::default();
        plan.half_length_m = -0.1;
        assert_eq!(search_contact(&plan, &truth, settings).unwrap_err(), "invalid_contact_execution_plan");
        plan.half_length_m = 0.16;
        plan.duration_sec = f64::NAN;
        assert_eq!(search_contact(&plan, &truth, settings).unwrap_err(), "invalid_contact_execution_plan");
        plan.duration_sec = 0.18;
        let mut bad = settings;
        bad.max_iterations = 0;
        assert_eq!(search_contact(&plan, &truth, bad).unwrap_err(), "invalid_contact_search_input");
    }
}
