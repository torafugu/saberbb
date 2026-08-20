use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::ball::{BallLocation, BallMovement, BallZone, PitchedBall};
use crate::domain::shared::player::PitcherInfo;
use crate::domain::shared::player::RL;
use crate::domain::util::GRAVITY;
use crate::domain::util::sigmoid;
use crate::error::AppError;

// TODO: Move to league pamater
const BASE_FOUR_SEAM_SPEED: f64 = 40.833; // 147.0 km/h;
const PITCH_OFFSET_DECISION_RATIO: f64 = 0.6;

// Pitch decelerates by approx. 8–10% from initial velocity due to air resistance by the time it reaches the mitt,
// so the average speed during flight is approx. 95% (0.95) of initial velocity
const AIR_DRAG_FACTOR: f64 = 0.95;

pub fn create_pitch(
    rng: &mut dyn RandomProvider,
    pitcher: &PitcherInfo,
) -> Result<PitchedBall, AppError> {
    // delivery form
    let base_spin_angle = pitcher.base_spin_angle();

    let pitch_call = pitcher.sample_pitch_calling(rng)?;
    let pitch_skill = pitcher.select_pitch_skill(pitch_call.pitch_type);

    let final_spin_angle = if pitcher.throw_side == RL::Left {
        (base_spin_angle - pitch_skill.spin_angle + 360.0) % 360.0
    } else {
        (base_spin_angle + pitch_skill.spin_angle + 360.0) % 360.0
    };

    let speed = pitcher.velocity * pitch_skill.velocity * rng.normal_factor_std_1_percent();

    // NOTE: Speed-based correction (slower pitches have lower spin rate)
    let speed_factor = speed / BASE_FOUR_SEAM_SPEED;

    let raw_spin_rate = pitcher.spin_rate
        * pitch_skill.spin_rate
        * rng.normal_factor_std_1_percent()
        * speed_factor;
    let release_point = pitcher.calculate_release_point(rng);
    let flight_time = calculate_flight_time(speed, release_point.y);

    let aim_location = pitch_call.aim_location();
    let actual_location = sample_ball_location(rng, pitch_call.target_zone.zone(), aim_location);

    Ok(PitchedBall {
        pitch_type: pitch_skill.pitch_type,
        speed,
        spin_rate: raw_spin_rate,
        spin_angle: final_spin_angle,
        spin_efficiency: pitch_skill.spin_efficiency,
        release_point: release_point,
        flight_time: flight_time,
        aim_zone: pitch_call.target_zone,
        aim_location: aim_location,
        actual_location: actual_location,
    })
}

// TODO: Consider pitcher's control effect.
// TODO: Consider ball location expetation.
fn sample_ball_location(
    rng: &mut dyn RandomProvider,
    zone: BallZone,
    aim: BallLocation,
) -> BallLocation {
    let norm_x = aim.x + zone.width() * rng.normal_std_10_percent();
    let norm_y = aim.y + zone.height() * rng.normal_std_10_percent();

    BallLocation {
        x: norm_x,
        y: norm_y,
    }
}

pub fn calculate_flight_time(speed: f64, release_point_y: f64) -> f64 {
    // 1. Calculate average pitch speed accounting for air resistance deceleration
    let avg_v_ms = speed * AIR_DRAG_FACTOR;

    // 2. Y-axis distance the ball actually travels (m)
    // (assumes release_point.y is already calculated as "18.44 - extension")
    let flight_distance_y = release_point_y;

    // 3. Flight time t = distance / average speed
    let flight_time = flight_distance_y / avg_v_ms;

    flight_time
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct PitchDisplacement {
    pub crossfire_multiplier: f64,
    pub release_x_factor: f64,
    pub horizontal_offset_m: f64,
    pub vertical_offset_m: f64,
    pub timing_offset_sec: f64,
}

pub fn calculate_ball_movement(ball: &PitchedBall) -> BallMovement {
    let flight_time = calculate_flight_time(ball.speed, ball.release_point.y);

    let movement_x_m = 0.5 * ball.get_side_accel() * flight_time.powi(2);
    let net_vertical_accel = ball.get_vertical_accel() - GRAVITY;
    let movement_z_m = 0.5 * net_vertical_accel * flight_time.powi(2);

    BallMovement {
        x_m: movement_x_m,
        z_m: movement_z_m,
    }
}

pub struct LocationBias {
    pub timing_bias_sec: f64,
    pub spatial_bias_x: f64,
    pub spatial_bias_y: f64,
}

// TODO: Reflect each batter's strengths and weaknesses with pitch location
pub fn calculate_location_bias(location: BallLocation) -> LocationBias {
    // 1. Inside (x < 0) tends to make batter late (+), Outside (x > 0) tends to make batter early (-)
    // Inside pitches jam the batter up to +0.012s (12ms), outside pitches can be waited on -0.010s
    let timing_bias = -location.x * 0.010 + location.y * 0.005;

    // 2. Lateral offset bias: Inside tends to jam (-), Outside tends to hit the end of the bat (+)
    let spatial_x = location.x * 0.20;

    // 3. Vertical offset bias: High (norm_y > 0) tends to hit above the ball (+), Low (norm_y < 0) tends to hit below (-)
    let spatial_y = location.y * 0.15;

    LocationBias {
        timing_bias_sec: timing_bias,
        spatial_bias_x: spatial_x,
        spatial_bias_y: spatial_y,
    }
}

pub fn calculate_pitch_offset(
    rng: &mut dyn RandomProvider,
    pitched_ball: &PitchedBall,
    expected_ball: &PitchedBall,
    matchup: &MatchupContext,
    location_bias: &LocationBias,
    batting_eye: f64,
) -> PitchDisplacement {
    // 1. Point at which the batter commits to the swing (approx. 60% of total flight time)
    let remaining_time = pitched_ball.flight_time * (1.0 - PITCH_OFFSET_DECISION_RATIO);

    // 2. Horizontal calculation
    let delta_horizontal = pitched_ball.get_side_accel() - expected_ball.get_side_accel();
    let offset_x = 0.5 * delta_horizontal * remaining_time.powi(2);

    // 3. Get the approach angle strength from lefty vs righty crossfire or wide release position (X)
    let crossfire_multiplier = matchup.crossfire_perceived_multiplier();

    // 4. Angle emphasis base proportional to release position magnitude (release_point.x)
    let release_x_factor = 1.0 + (pitched_ball.release_point.x.abs() * 0.15);

    // 5. Multiply the offset x by the crossfire illusion multiplier (correction)
    let enhanced_offset_x =
        offset_x * crossfire_multiplier * release_x_factor + location_bias.spatial_bias_x;

    // 6. Vertical calculation
    let delta_vertical = pitched_ball.get_vertical_accel() - expected_ball.get_vertical_accel();
    let offset_y = 0.5 * delta_vertical * remaining_time.powi(2);

    // 7. Apply vertical location bias
    let enhanced_offset_y = offset_y + location_bias.spatial_bias_y;

    // 8. Timing calculation
    let timing_offset_sec = calculate_timing_offset(rng, pitched_ball, expected_ball, batting_eye)
        + location_bias.timing_bias_sec;

    PitchDisplacement {
        crossfire_multiplier: crossfire_multiplier,
        release_x_factor: release_x_factor,
        horizontal_offset_m: enhanced_offset_x,
        vertical_offset_m: enhanced_offset_y,
        timing_offset_sec: timing_offset_sec,
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

// Calculate the Timing Offset (seconds) from the batter's prediction and the actual pitch
// NOTE: Timing when the batter initiates the swing (e.g. 12m from the pitcher / approx. 0.15s before impact)
fn calculate_timing_offset(
    rng: &mut dyn RandomProvider,
    pitched_ball: &PitchedBall,
    expected_ball: &PitchedBall,
    batting_eye: f64,
) -> f64 {
    // 1. Actual flight time (calculated from extension and actual pitch speed)
    let actual_release_point = pitched_ball.release_point.y * rng.normal_factor_std_1_percent();
    let actual_flight_time = calculate_flight_time(pitched_ball.speed, actual_release_point);

    // 2. Predicted flight time the batter calculates in their mind
    let release_point_seen_from_batter = actual_release_point
        * (1.0 + rng.normal_random(0.0, 0.01, 0.0, 1.0, 0.0) * sigmoid(batting_eye));
    let expected_flight_time =
        calculate_flight_time(expected_ball.speed, release_point_seen_from_batter);

    // 3. Adjustment based on pitch speed
    let speed_factor = (pitched_ball.speed / BASE_FOUR_SEAM_SPEED).min(1.0);

    // 4. Flight time difference (pure physical timing offset)
    // Sign convention for why raw_delta_t > 0 means late:
    // actual_time > expected_time (ball arrives slower) = batter swung too early (Early)
    // actual_time < expected_time (ball arrives faster) = batter swung late (Late)
    let delta_t_sec =
        (actual_flight_time - expected_flight_time) * sigmoid(batting_eye) * speed_factor;

    delta_t_sec
}

// TODO: half_height_m and half_height_m should be related with batter's height
pub struct StrikeZoneDimensions {
    pub half_width_m: f64,
    pub half_height_m: f64,
    pub center_height_m: f64,
}

impl Default for StrikeZoneDimensions {
    fn default() -> Self {
        Self {
            // Half width of home plate (m): 17 inches / 2 ≈ 0.216m
            half_width_m: 0.216,
            // Half height of the batter's strike zone (m): (Top - Bottom) / 2 (standard: approx. 0.325m)
            half_height_m: 0.325,
            // Height of the strike zone center (m): (Top + Bottom) / 2 (standard: approx. 0.825m)
            center_height_m: 0.825,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::random_provider::FixedRng;
    use crate::domain::shared::ball::BallLocation;
    use crate::domain::shared::player::{
        ArmSlot, FielderInfo, FielderType, PitchSkill, PitchType, PitcherStyle,
    };
    use crate::domain::strategy::pitching_strategy::TargetZone;
    use crate::domain::util::Vector3D;

    const EPSILON: f64 = 1e-9;
    const TEST_BATTING_EYE: f64 = 0.0;

    fn assert_near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "expected {actual} to be near {expected}"
        );
    }

    fn pitch_skill(pitch_type: PitchType, spin_angle: f64, spin_rate_rpm: f64) -> PitchSkill {
        let base_pitcher_spin_rate = 2200.0;

        PitchSkill::from_prob(
            pitch_type,
            spin_type_velocity(pitch_type),
            0.5,
            0.5,
            0.5,
            spin_rate_rpm / base_pitcher_spin_rate,
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
            2200.0,
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
                catching: 0.8,
                reach_height: 2.5,
                reach_range: 0.0,
            },
        )
    }

    fn pitched_ball(
        speed: f64,
        spin_rate: f64,
        spin_angle: f64,
        spin_efficiency: f64,
        release_x: f64,
        release_y: f64,
        flight_time: f64,
    ) -> PitchedBall {
        PitchedBall {
            pitch_type: PitchType::FourSeamFastball,
            speed,
            spin_rate,
            spin_angle,
            spin_efficiency,
            release_point: Vector3D {
                x: release_x,
                y: release_y,
                z: 1.7,
            },
            flight_time,
            aim_zone: TargetZone::Center,
            aim_location: BallLocation { x: 0.0, y: 0.0 },
            actual_location: BallLocation { x: 0.0, y: 0.0 },
        }
    }

    fn zero_location_bias() -> LocationBias {
        LocationBias {
            timing_bias_sec: 0.0,
            spatial_bias_x: 0.0,
            spatial_bias_y: 0.0,
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
        assert_near(ball.speed, 150.0);
        assert_near(ball.spin_rate, 2300.0);
        assert_near(ball.spin_angle, 75.0);
        assert_near(ball.spin_efficiency, 1.0);
        assert_near(ball.release_point.x, 0.55);
        assert_near(ball.release_point.y, 16.64);
        assert_near(ball.release_point.z, 1.71);
        assert_near(ball.flight_time, 16.64 / (150.0 * 0.95));
        assert_near(ball.aim_location.x, 0.75);
        assert_near(ball.aim_location.y, -0.75);
    }

    #[test]
    fn create_pitch_mirrors_spin_angle_and_release_for_left_hander() {
        let mut rng = FixedRng::new(0.0);
        let pitcher = pitcher(RL::Left, vec![pitch_skill(PitchType::Slider, 20.0, 2300.0)]);

        let ball = create_pitch(&mut rng, &pitcher).expect("pitch should be created");

        assert_eq!(ball.pitch_type, PitchType::Slider);
        assert_near(ball.speed, 132.0);
        assert_near(ball.spin_rate, 2024.0);
        assert_near(ball.spin_angle, 285.0);
        assert_near(ball.release_point.x, -0.55);
    }

    #[test]
    fn calculate_pitch_displacement_extracts_horizontal_spin_direction() {
        let right_break = pitched_ball(150.0, 2300.0, 90.0, 1.0, 0.0, 16.64, 0.42);
        let left_break = pitched_ball(150.0, 2300.0, 270.0, 1.0, 0.0, 16.64, 0.42);
        let flight_time = calculate_flight_time(150.0, 16.64);
        let expected_right_horizontal = 0.5 * right_break.get_side_accel() * flight_time.powi(2);
        let expected_right_vertical = 0.5 * right_break.get_vertical_accel() * flight_time.powi(2);
        let expected_left_horizontal = 0.5 * left_break.get_side_accel() * flight_time.powi(2);
        let expected_left_vertical = 0.5 * left_break.get_vertical_accel() * flight_time.powi(2);

        let right_displacement = calculate_ball_movement(&right_break);
        let left_displacement = calculate_ball_movement(&left_break);

        assert_near(right_displacement.x_m, expected_right_horizontal);
        assert_near(
            right_displacement.z_m,
            expected_right_vertical - 0.5 * GRAVITY * flight_time.powi(2),
        );
        assert_near(left_displacement.x_m, expected_left_horizontal);
        assert_near(
            left_displacement.z_m,
            expected_left_vertical - 0.5 * GRAVITY * flight_time.powi(2),
        );
    }

    #[test]
    fn calculate_pitch_displacement_returns_raw_physical_movement_for_extreme_spin() {
        let extreme_side_spin = pitched_ball(150.0, 9200.0, 90.0, 1.0, 0.0, 16.64, 0.42);
        let extreme_topspin = pitched_ball(150.0, 9200.0, 180.0, 1.0, 0.0, 16.64, 0.42);
        let flight_time = calculate_flight_time(150.0, 16.64);
        let expected_horizontal = 0.5 * extreme_side_spin.get_side_accel() * flight_time.powi(2);
        let expected_vertical = 0.5 * extreme_topspin.get_vertical_accel() * flight_time.powi(2);

        assert_near(
            calculate_ball_movement(&extreme_side_spin).x_m,
            expected_horizontal,
        );
        assert_near(
            calculate_ball_movement(&extreme_topspin).z_m,
            expected_vertical - 0.5 * GRAVITY * flight_time.powi(2),
        );
    }

    #[test]
    fn calculate_late_break_displacement_uses_remaining_acceleration_after_decision() {
        let mut rng = FixedRng::new(0.0);
        let ball = pitched_ball(150.0, 2500.0, 90.0, 1.0, 0.0, 16.64, 0.42);
        let expected_ball = pitched_ball(150.0, 2500.0, 0.0, 1.0, 0.0, 16.64, 0.42);
        let side_accel = ball.get_side_accel();
        let vertical_accel = ball.get_vertical_accel() - expected_ball.get_vertical_accel();
        let remaining_time = 0.42_f64 * 0.4;
        let expected_horizontal = 0.5 * side_accel * remaining_time.powi(2);
        let expected_vertical = 0.5 * vertical_accel * remaining_time.powi(2);

        let displacement = calculate_pitch_offset(
            &mut rng,
            &ball,
            &expected_ball,
            &MatchupContext {
                throw_side: RL::Right,
                batting_side: RL::Right,
            },
            &zero_location_bias(),
            TEST_BATTING_EYE,
        );

        assert_near(displacement.horizontal_offset_m, expected_horizontal);
        assert_near(displacement.vertical_offset_m, expected_vertical);
        assert_near(displacement.crossfire_multiplier, 1.0);
        assert_near(displacement.release_x_factor, 1.0);
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
    fn calculate_pitch_offset_enhances_horizontal_late_break_by_matchup_and_release_width() {
        let mut rng = FixedRng::new(0.0);
        let ball = pitched_ball(150.0, 2300.0, 90.0, 1.0, 0.80, 16.64, 0.42);
        let expected_ball = pitched_ball(150.0, 2300.0, 0.0, 1.0, 0.0, 16.64, 0.42);

        let same_side = calculate_pitch_offset(
            &mut rng,
            &ball,
            &expected_ball,
            &MatchupContext {
                throw_side: RL::Right,
                batting_side: RL::Right,
            },
            &zero_location_bias(),
            TEST_BATTING_EYE,
        );
        let crossfire = calculate_pitch_offset(
            &mut rng,
            &ball,
            &expected_ball,
            &MatchupContext {
                throw_side: RL::Left,
                batting_side: RL::Right,
            },
            &zero_location_bias(),
            TEST_BATTING_EYE,
        );

        assert!(crossfire.horizontal_offset_m > same_side.horizontal_offset_m);
        assert_near(crossfire.vertical_offset_m, same_side.vertical_offset_m);
        assert_near(same_side.crossfire_multiplier, 1.0);
        assert_near(crossfire.crossfire_multiplier, 1.30);
        assert_near(same_side.release_x_factor, 1.0 + 0.80 * 0.15);
        assert_near(crossfire.release_x_factor, same_side.release_x_factor);
    }

    #[test]
    fn calculate_location_bias_converts_pitch_location_to_timing_and_spatial_biases() {
        let bias = calculate_location_bias(BallLocation { x: -0.5, y: 0.4 });

        assert_near(bias.timing_bias_sec, 0.007);
        assert_near(bias.spatial_bias_x, -0.1);
        assert_near(bias.spatial_bias_y, 0.06);
    }

    #[test]
    fn calculate_pitch_offset_adds_location_bias_to_displacement() {
        let mut rng = FixedRng::new(0.0);
        let ball = pitched_ball(150.0, 2300.0, 0.0, 1.0, 0.0, 16.64, 0.42);
        let expected_ball = pitched_ball(150.0, 2300.0, 0.0, 1.0, 0.0, 16.64, 0.42);
        let location_bias = calculate_location_bias(BallLocation { x: 0.5, y: -0.4 });

        let displacement = calculate_pitch_offset(
            &mut rng,
            &ball,
            &expected_ball,
            &MatchupContext {
                throw_side: RL::Right,
                batting_side: RL::Right,
            },
            &location_bias,
            TEST_BATTING_EYE,
        );

        assert_near(
            displacement.horizontal_offset_m,
            location_bias.spatial_bias_x,
        );
        assert_near(displacement.vertical_offset_m, location_bias.spatial_bias_y);
        assert_near(
            displacement.timing_offset_sec,
            location_bias.timing_bias_sec,
        );
    }

    #[test]
    fn calculate_timing_offset_is_positive_for_slower_than_expected_pitch() {
        let mut rng = FixedRng::new(0.0);
        let ball = pitched_ball(135.0, 2300.0, 0.0, 1.0, 0.0, 1.75, 0.42);
        let expected_ball = pitched_ball(150.0, 2300.0, 0.0, 1.0, 0.0, 1.75, 0.42);

        assert!(calculate_timing_offset(&mut rng, &ball, &expected_ball, TEST_BATTING_EYE) > 0.0);
    }

    #[test]
    fn calculate_timing_offset_is_negative_for_faster_than_expected_pitch() {
        let mut rng = FixedRng::new(0.0);
        let ball = pitched_ball(160.0, 2300.0, 0.0, 1.0, 0.0, 1.75, 0.42);
        let expected_ball = pitched_ball(150.0, 2300.0, 0.0, 1.0, 0.0, 1.75, 0.42);

        assert!(calculate_timing_offset(&mut rng, &ball, &expected_ball, TEST_BATTING_EYE) < 0.0);
    }
}
