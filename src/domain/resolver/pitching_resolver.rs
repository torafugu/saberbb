use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::ball::PitchedBall;
use crate::domain::shared::player::RL;
use crate::domain::shared::player::{
    PITCH_EXTENSION_MAX, PITCH_EXTENSION_MIN, PitchType, PitcherInfo,
};
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

    let pitch_skill = pitcher.select_pitch_type(rng)?;

    let final_spin_angle = if pitcher.throw_side == RL::Left {
        (base_spin_angle - pitch_skill.spin_angle + 360.0) % 360.0
    } else {
        (base_spin_angle + pitch_skill.spin_angle + 360.0) % 360.0
    };

    let speed = pitcher.velocity * pitch_skill.velocity * rng.normal_factor_std_1percent();

    // Speed-based correction (slower pitches have lower spin rate)
    let speed_factor = speed / BASE_FOUR_SEAM_SPEED;

    let raw_spin_rate = pitch_skill.spin_rate * rng.normal_factor_std_1percent() * speed_factor;
    let release_point = pitcher.calculate_release_point();
    let flight_time = calculate_flight_time(speed, release_point.y);

    Ok(PitchedBall {
        pitch_type: pitch_skill.pitch_type,
        speed_kmh: speed,
        spin_rate: raw_spin_rate,
        spin_angle: final_spin_angle,
        spin_efficiency: pitch_skill.spin_efficiency,
        release_point: release_point,
        flight_time: flight_time,
        norm_x: 0.0,
        norm_y: 0.0,
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
pub fn calculate_late_break_displacement(ball: &PitchedBall) -> PitchDisplacement {
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

pub struct PitchedBallExpectation {
    pub pitch_type: PitchType,
    pub speed: f64,
    pub norm_x: f64,
    pub norm_y: f64,
}

/// Calculate the Timing Offset (seconds) from the batter's prediction and the actual pitch
pub fn calculate_timing_offset(
    ball: &PitchedBall,
    // Standard pitch profile the batter assumes in their mind (e.g. 150km/h fastball)
    expected_ball: &PitchedBallExpectation,
) -> f64 {
    // 1. Actual flight time (calculated from extension and actual pitch speed)
    let actual_flight_time = calculate_flight_time(ball.speed_kmh, ball.release_point.y);

    // 2. Predicted flight time the batter calculates in their mind
    // (calculated from the assumed pitch speed expected_speed and standard extension)
    const STANDARD_EXTENSION: f64 = (PITCH_EXTENSION_MIN + PITCH_EXTENSION_MAX) / 2.0;
    let expected_flight_time = calculate_flight_time(expected_ball.speed, STANDARD_EXTENSION);

    // 3. Flight time difference (pure physical timing offset)
    let raw_delta_t = actual_flight_time - expected_flight_time;

    // Sign convention for why raw_delta_t > 0 means late:
    // actual_time > expected_time (ball arrives slower) = batter swung too early (Early)
    // actual_time < expected_time (ball arrives faster) = batter swung late (Late)
    let delta_t_sec = raw_delta_t;

    delta_t_sec
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::random_provider::FixedRng;
    use crate::domain::shared::player::{
        ArmSlot, FielderInfo, FielderType, PitchSkill, PitcherStyle,
    };
    use crate::domain::util::Vector3D;

    const EPSILON: f64 = 1e-9;

    fn assert_near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "expected {actual} to be near {expected}"
        );
    }

    fn pitch_skill(pitch_type: PitchType, spin_angle: f64, spin_rate: f64) -> PitchSkill {
        PitchSkill::from_prob(
            pitch_type,
            spin_type_velocity(pitch_type),
            0.5,
            0.5,
            0.5,
            spin_rate,
            spin_angle,
            1.0,
            1.0,
        )
    }

    fn spin_type_velocity(pitch_type: PitchType) -> f64 {
        match pitch_type {
            PitchType::FourSeamFastball => 1.0,
            PitchType::Slider => 0.88,
            _ => 0.9,
        }
    }

    fn pitcher(throw_side: RL, pitch_skills: Vec<PitchSkill>) -> PitcherInfo {
        PitcherInfo::from_prob(
            1.80,
            1.80,
            throw_side,
            ArmSlot::ThreeQuarter,
            PitcherStyle::BalancedPitcher,
            150.0,
            0.5,
            0.5,
            0.5,
            0.5,
            0.0,
            0.0,
            1.5,
            pitch_skills,
            FielderInfo {
                fielder_type: FielderType::Pitcher,
                throw_speed: 40.0,
                running_speed: 7.0,
                reaction: 0.5,
                prep_time: 0.65,
            },
        )
    }

    fn pitched_ball(
        speed_kmh: f64,
        spin_rate: f64,
        spin_angle: f64,
        spin_efficiency: f64,
        release_x: f64,
        release_y: f64,
        flight_time: f64,
    ) -> PitchedBall {
        PitchedBall {
            pitch_type: PitchType::FourSeamFastball,
            speed_kmh,
            spin_rate,
            spin_angle,
            spin_efficiency,
            release_point: Vector3D {
                x: release_x,
                y: release_y,
                z: 1.7,
            },
            flight_time,
            norm_x: 0.0,
            norm_y: 0.0,
        }
    }

    #[test]
    fn create_pitch_uses_pitcher_pitch_skill_and_release_point() {
        let mut rng = FixedRng::new(0.0);
        let pitcher = pitcher(
            RL::Right,
            vec![pitch_skill(PitchType::FourSeamFastball, 20.0, 2300.0)],
        );

        let ball = create_pitch(&mut rng, &pitcher).expect("pitch should be created");

        assert_eq!(ball.pitch_type, PitchType::FourSeamFastball);
        assert_near(ball.speed_kmh, 150.0);
        assert_near(ball.spin_rate, 2300.0);
        assert_near(ball.spin_angle, 75.0);
        assert_near(ball.spin_efficiency, 1.0);
        assert_near(ball.release_point.x, 0.55);
        assert_near(ball.release_point.y, 16.64);
        assert_near(ball.release_point.z, 1.71);
        assert_near(ball.flight_time, 16.64 / ((150.0 / 3.6) * 0.95));
        assert_near(ball.norm_x, 0.0);
        assert_near(ball.norm_y, 0.0);
    }

    #[test]
    fn create_pitch_mirrors_spin_angle_and_release_for_left_hander() {
        let mut rng = FixedRng::new(0.0);
        let pitcher = pitcher(RL::Left, vec![pitch_skill(PitchType::Slider, 20.0, 2300.0)]);

        let ball = create_pitch(&mut rng, &pitcher).expect("pitch should be created");

        assert_eq!(ball.pitch_type, PitchType::Slider);
        assert_near(ball.speed_kmh, 132.0);
        assert_near(ball.spin_rate, 2024.0);
        assert_near(ball.spin_angle, 285.0);
        assert_near(ball.release_point.x, -0.55);
    }

    #[test]
    fn calculate_pitch_displacement_extracts_horizontal_spin_direction() {
        let right_break = pitched_ball(150.0, 2300.0, 90.0, 1.0, 0.0, 16.64, 0.42);
        let left_break = pitched_ball(150.0, 2300.0, 270.0, 1.0, 0.0, 16.64, 0.42);

        let right_displacement = calculate_pitch_displacement(&right_break);
        let left_displacement = calculate_pitch_displacement(&left_break);

        assert_near(right_displacement.horizontal, 0.5);
        assert_near(right_displacement.vertical, -0.5);
        assert_near(left_displacement.horizontal, -0.5);
        assert_near(left_displacement.vertical, -0.5);
    }

    #[test]
    fn calculate_pitch_displacement_clamps_extreme_spin() {
        let extreme_side_spin = pitched_ball(150.0, 9200.0, 90.0, 1.0, 0.0, 16.64, 0.42);
        let extreme_topspin = pitched_ball(150.0, 9200.0, 180.0, 1.0, 0.0, 16.64, 0.42);

        assert_near(
            calculate_pitch_displacement(&extreme_side_spin).horizontal,
            1.0,
        );
        assert_near(
            calculate_pitch_displacement(&extreme_topspin).vertical,
            -1.0,
        );
    }

    #[test]
    fn calculate_late_break_displacement_uses_remaining_acceleration_after_decision() {
        let ball = pitched_ball(150.0, 2500.0, 90.0, 1.0, 0.0, 16.64, 0.42);
        let side_accel = ball.get_side_accel();
        let total_vertical_accel = ball.get_vertical_accel() - GRAVITY;
        let expected_horizontal = (0.08 * side_accel * 0.42_f64.powi(2)) / 0.15;
        let expected_vertical = (0.08 * total_vertical_accel * 0.42_f64.powi(2)) / 0.15;

        let displacement = calculate_late_break_displacement(&ball);

        assert_near(displacement.horizontal, expected_horizontal);
        assert_near(displacement.vertical, expected_vertical);
    }

    #[test]
    fn matchup_context_weights_unfamiliar_crossfire_most_heavily() {
        assert_near(
            MatchupContext {
                throw_side: RL::Left,
                batting_side: RL::Right,
            }
            .crossfire_perceived_multiplier(),
            1.30,
        );
        assert_near(
            MatchupContext {
                throw_side: RL::Right,
                batting_side: RL::Left,
            }
            .crossfire_perceived_multiplier(),
            1.05,
        );
        assert_near(
            MatchupContext {
                throw_side: RL::Right,
                batting_side: RL::Right,
            }
            .crossfire_perceived_multiplier(),
            1.00,
        );
    }

    #[test]
    fn calculate_total_pitch_offset_enhances_horizontal_late_break_by_matchup_and_release_width() {
        let ball = pitched_ball(150.0, 2300.0, 90.0, 1.0, 0.80, 16.64, 0.42);
        let same_side = calculate_total_pitch_offset(
            &ball,
            &MatchupContext {
                throw_side: RL::Right,
                batting_side: RL::Right,
            },
        );
        let crossfire = calculate_total_pitch_offset(
            &ball,
            &MatchupContext {
                throw_side: RL::Left,
                batting_side: RL::Right,
            },
        );

        assert!(crossfire.horizontal > same_side.horizontal);
        assert_near(crossfire.vertical, same_side.vertical);
    }

    #[test]
    fn calculate_timing_offset_is_positive_for_slower_than_expected_pitch() {
        let ball = pitched_ball(135.0, 2300.0, 0.0, 1.0, 0.0, 1.75, 0.42);
        let expected_ball = PitchedBallExpectation {
            pitch_type: PitchType::FourSeamFastball,
            speed: 150.0,
            norm_x: 0.0,
            norm_y: 0.0,
        };

        assert!(calculate_timing_offset(&ball, &expected_ball) > 0.0);
    }

    #[test]
    fn calculate_timing_offset_is_negative_for_faster_than_expected_pitch() {
        let ball = pitched_ball(160.0, 2300.0, 0.0, 1.0, 0.0, 1.75, 0.42);
        let expected_ball = PitchedBallExpectation {
            pitch_type: PitchType::FourSeamFastball,
            speed: 150.0,
            norm_x: 0.0,
            norm_y: 0.0,
        };

        assert!(calculate_timing_offset(&ball, &expected_ball) < 0.0);
    }
}
