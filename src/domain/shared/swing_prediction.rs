//! Diagnostic forecast/plan inputs. No RNG and no effect on live decisions or outcomes.
use crate::domain::resolver::batting_resolver::{calculate_bat_angle, calculate_execution_angle_error_deg};
use crate::domain::resolver::pitching_resolver::StrikeZoneDimensions;
use crate::domain::shared::ball::{BallLocation, PitchedBall};
use crate::domain::shared::contact_state::ContactStateRecord;
use crate::domain::shared::player::BatterInfo;
use crate::domain::util::GRAVITY;
use serde::{Deserialize, Serialize};

pub const PREDICTION_SOURCE: &str = "expected_pitch_prior_commit_observation_v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation {
    pub time_sec: f64,
    pub position_world_m: [f64; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Forecast {
    pub time_sec: f64,
    pub position_world_m: [f64; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwingPredictionInput {
    pub observation: Observation,
    pub estimated_velocity_world_ms: [f64; 3],
    pub estimated_acceleration_world_ms2: [f64; 3],
    pub forecast: Forecast,
    pub prior_flight_time_sec: f64,
    pub prior_speed_ms: f64,
    pub commit_ratio: f64,
    pub planned_bat_angle_deg: f64,
    pub planned_attack_angle_deg: f64,
    pub execution_angle_error_deg: f64,
    pub bat_speed_ms: f64,
    pub handedness_sign: f64,
    pub batting_eye: f64,
    pub bat_control: f64,
    pub zone_modifier_at_prediction: f64,
    // Old timing already contains forecast error; do not add it to this plan.
    pub residual_execution_delay_sec: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwingPredictionRecord {
    pub version: u8,
    pub source: String,
    pub invalid_reason: Option<String>,
    pub input: Option<SwingPredictionInput>,
}

/// Deliberately has no actual plate position/time or live contact outcome argument.
pub fn forecast_plate_crossing(
    observation: &Observation,
    estimated_velocity: [f64; 3],
    estimated_acceleration: [f64; 3],
) -> Result<Forecast, &'static str> {
    if !observation.time_sec.is_finite() || observation.time_sec < 0.0
        || !observation.position_world_m.iter().chain(estimated_velocity.iter())
            .chain(estimated_acceleration.iter()).all(|x| x.is_finite())
        || observation.position_world_m[1] <= 0.0 || estimated_velocity[1] >= 0.0
        || estimated_acceleration[1].abs() > 1e-12
    {
        return Err("invalid_forecast_observation_or_velocity");
    }
    let dt = -observation.position_world_m[1] / estimated_velocity[1];
    let position: [f64; 3] = std::array::from_fn(|i|
        observation.position_world_m[i] + estimated_velocity[i] * dt
            + 0.5 * estimated_acceleration[i] * dt * dt);
    let time_sec = observation.time_sec + dt;
    if !time_sec.is_finite() || !position.iter().all(|x| x.is_finite()) {
        return Err("non_finite_forecast");
    }
    Ok(Forecast { time_sec, position_world_m: position })
}

fn acceleration(ball: &PitchedBall) -> [f64; 3] {
    [ball.get_side_accel(), 0.0, ball.get_vertical_accel() - GRAVITY]
}

/// Simulation truth is sampled here at commit, then reduced to a position-only observation.
/// Estimated velocity/acceleration come from the already sampled expected-ball prior.
fn collect_input(batter: &BatterInfo, actual: &PitchedBall, prior: &PitchedBall)
    -> Result<SwingPredictionInput, &'static str>
{
    let zone = StrikeZoneDimensions::default();
    if !prior.flight_time.is_finite() || prior.flight_time <= 0.0
        || !actual.flight_time.is_finite() || actual.flight_time <= 0.0 {
        return Err("invalid_observation_clock");
    }
    let commit_ratio = 0.6;
    let commit = prior.flight_time * commit_ratio;
    if commit >= actual.flight_time { return Err("commit_at_or_after_actual_plate"); }
    let av = actual.velocity_at_plate(&zone).ok_or("invalid_actual_velocity")?;
    let pv = prior.velocity_at_plate(&zone).ok_or("invalid_prior_velocity")?;
    let aa = acceleration(actual);
    let pa = acceleration(prior);
    let actual_plate_velocity = [av.x, av.y, av.z];
    let prior_plate_velocity = [pv.x, pv.y, pv.z];
    let release = [actual.release_point.x, actual.release_point.y, actual.release_point.z];
    let observed: [f64; 3] = std::array::from_fn(|i| release[i]
        + (actual_plate_velocity[i] - aa[i] * actual.flight_time) * commit
        + 0.5 * aa[i] * commit * commit);
    let estimate: [f64; 3] = std::array::from_fn(|i|
        prior_plate_velocity[i] + pa[i] * (commit - prior.flight_time));
    let observation = Observation { time_sec: commit, position_world_m: observed };
    let forecast = forecast_plate_crossing(&observation, estimate, pa)?;
    let predicted_location = BallLocation {
        x: forecast.position_world_m[0] / zone.half_width_m,
        y: (forecast.position_world_m[2] - zone.center_height_m) / zone.half_height_m,
    };
    let execution_error = calculate_execution_angle_error_deg(batter, &predicted_location);
    let zone_modifier = batter.zone_modifier(&predicted_location);
    let values = [batter.attack_angle, batter.swing_speed, batter.batting_eye, batter.bat_control,
        execution_error, prior.speed, zone_modifier];
    if !values.iter().all(|x| x.is_finite()) || batter.swing_speed < 0.0 || prior.speed <= 0.0 || batter.attack_angle.abs() > 90.0 {
        return Err("invalid_plan_batter_parameters");
    }
    Ok(SwingPredictionInput {
        observation, estimated_velocity_world_ms: estimate,
        estimated_acceleration_world_ms2: pa, forecast,
        prior_flight_time_sec: prior.flight_time, prior_speed_ms: prior.speed, commit_ratio,
        planned_bat_angle_deg: calculate_bat_angle(&predicted_location),
        planned_attack_angle_deg: batter.attack_angle,
        execution_angle_error_deg: execution_error,
        bat_speed_ms: batter.swing_speed,
        handedness_sign: batter.batting_side.handedness_sign(),
        batting_eye: batter.batting_eye, bat_control: batter.bat_control,
        zone_modifier_at_prediction: zone_modifier,
        residual_execution_delay_sec: 0.0,
    })
}

pub fn attach_prediction(record: &mut ContactStateRecord, batter: &BatterInfo,
    actual: &PitchedBall, prior: &PitchedBall)
{
    if let Some(state) = record.state.as_mut() {
        let (input, invalid_reason) = match collect_input(batter, actual, prior) {
            Ok(input) => (Some(input), None),
            Err(reason) => (None, Some(reason.to_string())),
        };
        state.swing_prediction = Some(SwingPredictionRecord {
            version: 1, source: PREDICTION_SOURCE.to_string(), input, invalid_reason,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forecast_uses_only_commit_observation_and_estimates() {
        let observed = Observation { time_sec: 0.24, position_world_m: [0.1, 6.0, 1.5] };
        let a = forecast_plate_crossing(&observed, [1.0, -40.0, -2.0], [0.0, 0.0, -10.0]).unwrap();
        assert!((a.time_sec - 0.39).abs() < 1e-12);
        assert!((a.position_world_m[0] - 0.25).abs() < 1e-12);
        assert!((a.position_world_m[1]).abs() < 1e-12);
        assert!((a.position_world_m[2] - 1.0875).abs() < 1e-12);
        assert!(forecast_plate_crossing(&observed, [1.0, 0.0, -2.0], [0.0; 3]).is_err());
        assert!(forecast_plate_crossing(&observed, [1.0, -40.0, -2.0], [0.0, 1.0, 0.0]).is_err());
    }
}
