//! Diagnostic-only contact states on one release-relative clock. No RNG or outcome changes.
use crate::domain::resolver::batting_resolver::SwingContactResult;
use crate::domain::resolver::pitching_resolver::StrikeZoneDimensions;
use crate::domain::shared::ball::PitchedBall;
use crate::domain::shared::player::BatterInfo;
use serde::{Deserialize, Serialize};

pub const CONTACT_STATE_VERSION: u8 = 1;
pub const CONTACT_STATE_SOURCE: &str = "plate_constant_accel_linear_bat_world_aim_proxy_v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContactStateRecord {
    pub count_seq: u16,
    pub state_version: u8,
    pub state_source: String,
    pub invalid_reason: Option<String>,
    pub state: Option<ContactStateSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContactStateInput {
    pub reference_time_sec_since_release: f64,
    pub ball_position_world_m: [f64; 3],
    pub ball_velocity_world_ms: [f64; 3],
    pub bat_speed_ms: f64,
    pub bat_angle_deg: f64,
    pub attack_angle_deg: f64,
    pub handedness_sign: f64,
    /// Adapted spatial errors are a world-X/Z aiming proxy, not measured positions.
    pub aim_error_world_m: [f64; 3],
    /// Positive is late. Applied once as a delay to the linear bat trajectory.
    pub timing_offset_sec: f64,
    pub legacy_timing_impact_x_m: f64,
    pub legacy_contact_offset_xz_m: [f64; 2],
    pub legacy_timing_position_scale: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContactStateSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_intent_plans: Option<crate::domain::shared::observation_update::ObservedIntentDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_plans: Option<crate::domain::shared::forecast_swing_plan::IntentPlanDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swing_prediction: Option<crate::domain::shared::swing_prediction::SwingPredictionRecord>,
    pub input: ContactStateInput,
    pub bat_ideal_position_world_m: [f64; 3],
    pub bat_position_world_m: [f64; 3],
    pub bat_axis_world: [f64; 3],
    pub bat_velocity_world_ms: [f64; 3],
    pub bat_angular_velocity_world_rad_s: [f64; 3],
    pub relative_position_world_m: [f64; 3],
    pub relative_velocity_world_ms: [f64; 3],
}

pub fn snapshot(input: ContactStateInput) -> Result<ContactStateSnapshot, &'static str> {
    let scalars = [input.reference_time_sec_since_release, input.bat_speed_ms,
        input.bat_angle_deg, input.attack_angle_deg, input.handedness_sign,
        input.timing_offset_sec, input.legacy_timing_impact_x_m];
    if !scalars.iter().all(|x| x.is_finite())
        || !input.ball_position_world_m.iter().chain(input.ball_velocity_world_ms.iter())
            .chain(input.aim_error_world_m.iter()).chain(input.legacy_contact_offset_xz_m.iter())
            .all(|x| x.is_finite())
        || input.legacy_timing_position_scale.is_some_and(|x| !x.is_finite() || x <= 0.0)
    {
        return Err("non_finite_or_invalid_input");
    }
    if input.reference_time_sec_since_release <= 0.0 || input.bat_speed_ms < 0.0
        || input.attack_angle_deg.abs() > 90.0
        || ![-1.0, 1.0].contains(&input.handedness_sign)
    {
        return Err("invalid_reference_time_speed_angle_or_side");
    }
    let attack = input.attack_angle_deg.to_radians();
    let angle = input.bat_angle_deg.to_radians();
    let velocity = [0.0, input.bat_speed_ms * attack.cos(), input.bat_speed_ms * attack.sin()];
    let ideal: [f64; 3] = std::array::from_fn(|i| input.ball_position_world_m[i] - input.aim_error_world_m[i]);
    // bat(t_ref) = ideal(t_ref - delay). Do not add legacy timing_impact_x_m again.
    let bat: [f64; 3] = std::array::from_fn(|i| ideal[i] - velocity[i] * input.timing_offset_sec);
    let relative: [f64; 3] = std::array::from_fn(|i| input.aim_error_world_m[i] + velocity[i] * input.timing_offset_sec);
    let relative_velocity: [f64; 3] = std::array::from_fn(|i| input.ball_velocity_world_ms[i] - velocity[i]);
    if !bat.iter().chain(relative.iter()).chain(relative_velocity.iter()).all(|x: &f64| x.is_finite()) {
        return Err("non_finite_derived_state");
    }
    Ok(ContactStateSnapshot {
        observed_intent_plans: None,
        intent_plans: None,
        swing_prediction: None,
        bat_ideal_position_world_m: ideal,
        bat_position_world_m: bat,
        bat_axis_world: [input.handedness_sign * angle.cos(), 0.0, angle.sin()],
        bat_velocity_world_ms: velocity,
        bat_angular_velocity_world_rad_s: [0.0; 3],
        relative_position_world_m: relative,
        relative_velocity_world_ms: relative_velocity,
        input,
    })
}

pub fn build_contact_state(
    count_seq: u16,
    batter: &BatterInfo,
    ball: &PitchedBall,
    contact: &SwingContactResult,
) -> ContactStateRecord {
    let result = (|| {
        let zone = StrikeZoneDimensions::default();
        let velocity = ball.velocity_at_plate(&zone).ok_or("invalid_pitch_flight_time")?;
        let angle = contact.actual_bat_angle_deg.ok_or("missing_actual_bat_angle")?;
        snapshot(ContactStateInput {
            reference_time_sec_since_release: ball.flight_time,
            ball_position_world_m: [ball.actual_location.x * zone.half_width_m, 0.0,
                zone.center_height_m + ball.actual_location.y * zone.half_height_m],
            ball_velocity_world_ms: [velocity.x, velocity.y, velocity.z],
            bat_speed_ms: batter.swing_speed,
            bat_angle_deg: angle,
            attack_angle_deg: contact.attack_angle_deg,
            handedness_sign: batter.batting_side.handedness_sign(),
            aim_error_world_m: [contact.adapted_horizontal_offset_m + contact.swing_error_x_m,
                0.0, contact.adapted_vertical_offset_m + contact.swing_error_z_m],
            timing_offset_sec: contact.timing_offset_sec,
            legacy_timing_impact_x_m: contact.timing_impact_x_m,
            legacy_contact_offset_xz_m: [contact.offset_x_m, contact.offset_z_m],
            legacy_timing_position_scale: contact.timing_position_scale,
        })
    })();
    let (state, invalid_reason) = match result {
        Ok(state) => (Some(state), None),
        Err(reason) => (None, Some(reason.to_string())),
    };
    ContactStateRecord { count_seq, state_version: CONTACT_STATE_VERSION,
        state_source: CONTACT_STATE_SOURCE.to_string(), invalid_reason, state }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(delay: f64) -> ContactStateInput {
        ContactStateInput {
            reference_time_sec_since_release: 0.4,
            ball_position_world_m: [0.02, 0.0, 0.9],
            ball_velocity_world_ms: [1.0, -38.0, -2.0],
            bat_speed_ms: 35.0, bat_angle_deg: 30.0, attack_angle_deg: 10.0,
            handedness_sign: 1.0, aim_error_world_m: [0.003, 0.0, -0.002],
            timing_offset_sec: delay, legacy_timing_impact_x_m: 0.1,
            legacy_contact_offset_xz_m: [0.103, -0.002], legacy_timing_position_scale: Some(0.75),
        }
    }

    #[test]
    fn optional_prediction_preserves_old_json_and_new_payloads() {
        let state = snapshot(input(0.0)).unwrap();
        let old = serde_json::to_value(&state).unwrap();
        assert!(old.get("swing_prediction").is_none());
        assert!(old.get("intent_plans").is_none());
        assert!(old.get("observed_intent_plans").is_none());
        let decoded: ContactStateSnapshot = serde_json::from_value(old).unwrap();
        assert!(decoded.swing_prediction.is_none());
        assert!(decoded.intent_plans.is_none());
        assert!(decoded.observed_intent_plans.is_none());
        let mut state = state;
        state.swing_prediction = Some(crate::domain::shared::swing_prediction::SwingPredictionRecord {
            version: 1,
            source: crate::domain::shared::swing_prediction::PREDICTION_SOURCE.to_string(),
            invalid_reason: Some("invalid_observation_clock".to_string()), input: None,
        });
        let encoded = serde_json::to_string(&state).unwrap();
        let decoded: ContactStateSnapshot = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.swing_prediction.unwrap().invalid_reason.as_deref(), Some("invalid_observation_clock"));
    }

    #[test]
    fn attaching_prediction_records_intent_plans_without_changing_contact_state() {
        use crate::domain::shared::ball::BallLocation;
        use crate::domain::shared::player::{PitchType, RL};
        use crate::domain::strategy::pitching_strategy::TargetZone;
        use crate::domain::util::Vector3D;
        let state = snapshot(input(0.0)).unwrap();
        let before = serde_json::to_value(&state).unwrap();
        let mut record = ContactStateRecord { count_seq: 1, state_version: 1,
            state_source: CONTACT_STATE_SOURCE.into(), invalid_reason: None, state: Some(state) };
        let ball = PitchedBall { pitch_type: PitchType::FourSeamFastball, speed: 40.0, spin_rate: 2200.0,
            spin_angle: 0.0, spin_efficiency: 0.9, release_point: Vector3D { x: 0.0, y: 16.0, z: 1.8 },
            flight_time: 0.4, aim_zone: TargetZone::Center,
            aim_location: BallLocation { x: 0.0, y: 0.0 }, actual_location: BallLocation { x: 0.0, y: 0.0 } };
        let batter = crate::domain::test_support::batter_info(RL::Right);
        crate::domain::shared::swing_prediction::attach_prediction(&mut record, &batter, &ball, &ball);
        let decision = crate::domain::strategy::batting_strategy::decide_batting_intent(
            crate::domain::strategy::batting_strategy::BattingStrategy::SeekLineDrive,
            crate::domain::strategy::batting_strategy::BattingPreferences::default());
        crate::domain::shared::observation_update::attach_observed_intent_plans(&mut record, 42, decision);
        let state = record.state.unwrap();
        assert_eq!(state.intent_plans.as_ref().unwrap().plans.len(), 2);
        let observed = state.observed_intent_plans.as_ref().unwrap();
        assert!(observed.invalid_reason.is_none());
        assert_eq!(observed.decision, decision);
        assert_eq!(observed.intent_plans.as_ref().unwrap().plans.len(), 2);
        let encoded = serde_json::to_value(&state).unwrap();
        let decoded: ContactStateSnapshot = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(decoded.intent_plans.unwrap().plans.len(), 2);
        assert_eq!(decoded.observed_intent_plans.unwrap().decision, decision);
        let mut existing_fields = encoded;
        existing_fields.as_object_mut().unwrap().remove("swing_prediction");
        existing_fields.as_object_mut().unwrap().remove("intent_plans");
        existing_fields.as_object_mut().unwrap().remove("observed_intent_plans");
        assert_eq!(existing_fields, before);
    }

    #[test]
    fn timing_is_one_phase_delay_not_an_x_addition() {
        for delay in [-0.003, 0.0, 0.003] {
            let state = snapshot(input(delay)).unwrap();
            assert!((state.relative_position_world_m[0] - 0.003).abs() < 1e-12);
            for i in 0..3 {
                assert!((state.input.ball_position_world_m[i] - state.bat_position_world_m[i]
                    - state.relative_position_world_m[i]).abs() < 1e-12);
            }
            assert!((state.relative_position_world_m[1] - state.bat_velocity_world_ms[1]*delay).abs() < 1e-12);
        }
    }

    #[test]
    fn mirror_and_invalid_inputs_are_explicit() {
        let right = snapshot(input(0.002)).unwrap();
        let mut left_input = input(0.002);
        left_input.handedness_sign = -1.0;
        left_input.ball_position_world_m[0] *= -1.0;
        left_input.ball_velocity_world_ms[0] *= -1.0;
        left_input.aim_error_world_m[0] *= -1.0;
        let left = snapshot(left_input).unwrap();
        assert_eq!(left.relative_position_world_m[0], -right.relative_position_world_m[0]);
        assert_eq!(left.bat_axis_world[0], -right.bat_axis_world[0]);
        assert_eq!(left.relative_position_world_m[2], right.relative_position_world_m[2]);
        let mut bad = input(0.0);
        bad.reference_time_sec_since_release = f64::NAN;
        assert!(snapshot(bad).is_err());
    }
}
