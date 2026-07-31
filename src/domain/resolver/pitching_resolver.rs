use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::ball::PitchedBall;
use crate::domain::shared::player::PitcherInfo;
use crate::domain::shared::player::RL;
use crate::domain::util::GRAVITY;
use crate::error::AppError;
use std::f64::consts::PI;

const BASE_FOUR_SEAM_SPEED: f64 = 150.0;

// Pitch decelerates by approx. 8–10% from initial velocity due to air resistance by the time it reaches the mitt,
// so the average speed during flight is approx. 95% (0.95) of initial velocity
const AIR_DRAG_FACTOR: f64 = 0.95;

// Scaling: 2300rpm = 1.0 Magnus force
const MAGNUS_FORCE_BASE_SPIN_RATE: f64 = 2300.0;

pub fn create_pitch(
    rng: &mut dyn RandomProvider,
    pitcher: &PitcherInfo,
) -> Result<PitchedBall, AppError> {
    // delivery form
    let base_spin_angle = pitcher.base_spin_angle();

    let pitch_skill = pitcher.select_pitch_skill(rng)?;

    let final_spin_angle = if pitcher.throw_side == RL::Left {
        (base_spin_angle - pitch_skill.spin_angle + 360.0) % 360.0
    } else {
        (base_spin_angle + pitch_skill.spin_angle + 360.0) % 360.0
    };

    let speed = pitch_skill.velocity * rng.normal_factor_std_1percent();

    // Speed-based correction (slower pitches have lower spin rate)
    let speed_factor = speed / BASE_FOUR_SEAM_SPEED;

    let raw_spin_rate = pitch_skill.spin_rate * rng.normal_factor_std_1percent() * speed_factor;
    let release_point = pitcher.calculate_release_point();
    let flight_time = calculate_flight_time(speed, release_point.y);

    Ok(PitchedBall {
        speed: speed,
        spin_rate: raw_spin_rate,
        spin_angle: final_spin_angle,
        spin_efficiency: pitch_skill.spin_efficiency,
        release_point: release_point,
        flight_time: flight_time,
    })
}

/// Calculate the flight time (seconds) for the pitch to reach the batter (impact point)
fn calculate_flight_time(speed: f64, release_point_y: f64) -> f64 {
    // 1. Convert pitch speed (km/h) to (m/s)
    let initial_v_ms = speed / 3.6;

    // 2. Calculate average pitch speed accounting for air resistance deceleration
    let avg_v_ms = initial_v_ms * AIR_DRAG_FACTOR;

    // 3. Y-axis distance the ball actually travels (m)
    // (assumes release_point.y is already calculated as "18.44 - extension")
    let flight_distance_y = release_point_y;

    // 4. Flight time t = distance / average speed
    let flight_time = flight_distance_y / avg_v_ms;

    flight_time
}

pub struct PitchDisplacement {
    // NOTE: Horizontal offset (-1.0: shift left ~ +1.0: shift right)
    pub horizontal: f64,
    // NOTE: Vertical offset (-1.0: shift down ~ +1.0: shift up)
    pub vertical: f64,
}

// NOTE: Calculate sweet_spot_factor (0.0 ~ 1.0) solely from pitch spin characteristics
pub fn calculate_pitch_displacement(ball: &PitchedBall) -> PitchDisplacement {
    // 1. Simple calculation of Magnus effect magnitude based on spin rate
    let magnus_strength = ball.spin_rate / MAGNUS_FORCE_BASE_SPIN_RATE;

    // Convert spin angle (degrees) to radians
    let angle_rad = ball.spin_angle * PI / 180.0;

    // 2. Calculate movement vector
    // X: horizontal movement (screw/slider)
    // sin(spin_angle): 90 deg (slider) => +1.0 (right), 270 deg (screw) => -1.0 (left)
    let delta_x = magnus_strength * angle_rad.sin();

    // Y: vertical movement (backspin/topspin)
    // Use a standard fastball (spin_angle = 0.0) backspin component (1.0) as the reference point, measuring deviation from it
    let fastball_ref_y = 1.0;
    let delta_y = (magnus_strength * angle_rad.cos()) - fastball_ref_y;

    // 3. Scale adjustment based on bat sweet spot size (sensitivity parameter)
    // Coefficient to map offset to -1.0 ~ +1.0 range (sweet spot ~ bat edge)
    let sensitivity = 0.5;

    PitchDisplacement {
        horizontal: (delta_x * sensitivity).clamp(-1.0, 1.0),
        vertical: (delta_y * sensitivity).clamp(-1.0, 1.0),
    }
}

/// Timing when the batter initiates the swing (e.g. 12m from the pitcher / approx. 0.15s before impact)
fn calculate_late_break_displacement(ball: &PitchedBall) -> PitchDisplacement {
    // Point at which the batter commits to the swing (approx. 60% of total flight time)
    let decision_ratio = 0.6;
    let decision_time = ball.flight_time * decision_ratio;

    // 1. Horizontal calculation
    let side_accel = ball.get_side_accel();

    // Position and velocity at the decision point (t_decision)
    let x_at_decision = 0.5 * side_accel * decision_time.powi(2);
    let v_x_at_decision = side_accel * decision_time;

    // Brain predicts the ball continues straight at current velocity for the remaining time (flight_time - decision_time)
    let remaining_time = ball.flight_time - decision_time;
    let x_predicted = x_at_decision + (v_x_at_decision * remaining_time);

    // Actual impact position (result of t² acceleration applied for the entire flight)
    let x_actual = 0.5 * side_accel * ball.flight_time.powi(2);
    let late_break_x = x_actual - x_predicted;

    // 2. Vertical calculation
    let spin_vertical_accel = ball.get_vertical_accel();
    let total_vertical_accel = spin_vertical_accel - GRAVITY; // Magnus lift - gravity

    // Position and vertical velocity at the decision point
    let y_at_decision = 0.5 * total_vertical_accel * decision_time.powi(2);
    let v_y_at_decision = total_vertical_accel * decision_time;

    // Predicted position extending the trajectory tangent (straight line) the batter perceived at the decision point to impact
    let y_predicted = y_at_decision + (v_y_at_decision * remaining_time);

    // Actual impact position
    let y_actual = 0.5 * total_vertical_accel * ball.flight_time.powi(2);

    // Difference between predicted and actual position (the amount perceived as abrupt sink/rise at the plate)
    let late_break_y = y_actual - y_predicted;

    // 3. Map to -1.0 ~ +1.0 offset scale (scaling)
    // Example: 15cm (0.15m) of break is defined as full offset (1.0)
    const DISPLACEMENT_SENSITIVITY: f64 = 1.0 / 0.15;

    PitchDisplacement {
        horizontal: (late_break_x * DISPLACEMENT_SENSITIVITY).clamp(-1.0, 1.0),
        vertical: (late_break_y * DISPLACEMENT_SENSITIVITY).clamp(-1.0, 1.0),
    }
}

pub struct MatchupContext {
    pub throw_side: RL,
    pub batting_side: RL,
}

impl MatchupContext {
    /// Perceived crossfire multiplier based on pitcher/batter handedness matchup
    pub fn crossfire_perceived_multiplier(&self) -> f64 {
        match (self.throw_side, self.batting_side) {
            // Left-handed pitcher vs right-handed batter: least familiar, comes from the blind side, so the effect is amplified
            (RL::Left, RL::Right) => 1.30,

            // Right-handed pitcher vs left-handed batter: physically the same diagonal, but left-handed batters are used to right-handed pitchers, so standard
            (RL::Right, RL::Left) => 1.05,

            // Same side (right vs right, left vs left): only the arm angle (blind side) intimidation, no diagonal
            _ => 1.00,
        }
    }
}

/// Combine the pitch-side component of the final ContactOffset from pitch info and batter context
pub fn calculate_total_pitch_offset(
    ball: &PitchedBall,
    matchup: &MatchupContext,
) -> PitchDisplacement {
    // 1. Base displacement from the sweet spot from total physical movement
    let base_disp = calculate_pitch_displacement(ball);

    // 2. Late break amount at the plate from the t² effect
    let late_break = calculate_late_break_displacement(ball);

    // 3. Apply visual illusion multiplier from crossfire / arm slot
    // Get the approach angle strength from lefty vs righty crossfire or wide release position (X)
    let crossfire_multiplier = matchup.crossfire_perceived_multiplier();

    // Angle emphasis base proportional to release position magnitude (release_point.x)
    let release_x_factor = 1.0 + (ball.release_point.x.abs() * 0.15);

    // Multiply the late break (late_break.horizontal) by the crossfire illusion multiplier (correction)
    let enhanced_late_break_x = late_break.horizontal * crossfire_multiplier * release_x_factor;

    // 4. Combine base displacement and late-break illusion displacement
    PitchDisplacement {
        horizontal: (base_disp.horizontal + enhanced_late_break_x).clamp(-1.0, 1.0),
        vertical: (base_disp.vertical + late_break.vertical).clamp(-1.0, 1.0),
    }
}
