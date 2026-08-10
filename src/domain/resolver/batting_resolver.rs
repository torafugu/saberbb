use crate::domain::resolver::pitching_resolver::PitchDisplacement;
use crate::domain::shared::ball::{BallLocation, BattedBall, PitchedBall, TrajectoryType};
use crate::domain::shared::player::{BatterInfo, RL};
use crate::domain::util::{CONVERT_FACTOR_MS_TO_KMH, GRAVITY};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use strum_macros::{AsRefStr, EnumIter, EnumString};

// Standard reference swing speed (km/h)
const REF_SWING_SPEED: f64 = 120.0;
// Maximum spin rate generated when fully brushing the ball at reference swing (rpm)
const MAX_COLLISION_SPIN_AT_REF_SPEED: f64 = 4000.0;

fn calculate_bat_angle(location: &BallLocation) -> f64 {
    const CENTER_ANGLE_DEG: f64 = 30.0; // Standard tilt angle at zone center
    const HIGH_LOW_RANGE_DEG: f64 = 15.0; // Angle variation range based on height

    // Map norm_y (+1.0 high ~ -1.0 low) to 15° ~ 45°
    let base_angle = CENTER_ANGLE_DEG - (location.y * HIGH_LOW_RANGE_DEG);

    // Clamp to human range of motion limits (10° ~ 60°)
    base_angle.clamp(10.0, 60.0)
}

pub struct SwingExecutionError {
    pub additional_x_m: f64,
    pub additional_z_m: f64,
    pub actual_bat_angle_deg: f64,
}

// Calculate the actual bat_angle_deg the batter swings through based on the real trajectory and their prediction
pub fn calculate_swing_execution_error(
    bat_contact: f64,
    intended_location: &BallLocation,
    actual_location: &BallLocation,
) -> SwingExecutionError {
    // 1. Calculate the difference between intended and ideal angle (angle error)
    let intended_angle = calculate_bat_angle(intended_location);
    let ideal_angle = calculate_bat_angle(actual_location);

    // Lower contact skill leaves a larger Δθ because the swing can't correct toward the ideal angle
    let unadjusted_ratio = (1.0 - bat_contact).clamp(0.1, 1.0);
    let delta_angle_deg = (intended_angle - ideal_angle) * unadjusted_ratio;

    // 2. Convert angle error (deg) to spatial meter error (Δx, Δz)
    let delta_rad = delta_angle_deg.to_radians();
    const BAT_BARREL_LENGTH_M: f64 = 0.70; // Distance from grip to sweet spot

    let additional_z_m = BAT_BARREL_LENGTH_M * delta_rad.sin();
    let additional_x_m = BAT_BARREL_LENGTH_M * (1.0 - delta_rad.cos());

    // Actual bat angle the batter swung
    let actual_bat_angle_deg = intended_angle - (intended_angle - ideal_angle) * bat_contact;

    SwingExecutionError {
        additional_x_m,
        additional_z_m,
        actual_bat_angle_deg,
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, EnumString, Serialize, Deserialize, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum PitchOutcome {
    InPlay,
    Foul,
    StrikeSwung,
    StrikeLooking,
    Ball,
}

// Mismatch between the batter's swing prediction and the actual pitch
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwingContactResult {
    pub offset_x_m: f64,
    pub offset_z_m: f64,
    // NOTE: Spatial sweet-spot offset (0.0: perfectly centered ~ 1.0: completely missing the zone)
    pub thickness_offset_m: f64,
    pub length_offset_m: f64,
    // TODO: timing_offset must be removed.
    pub timing_offset: f64,
    pub contact_type: SwingContactType,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, EnumString, Serialize, Deserialize, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum SwingContactType {
    // NOTE: Caught it on the sweet spot (likely fair / extra-base hit)
    SolidContact,
    // NOTE: Missed the sweet spot (likely grounder / fly / foul)
    WeakContact,
    // NOTE: Barely grazed it (tip foul)
    FoulTip,
    // NOTE: Bat swung through air completely (swing and miss)
    SwungAndMiss,
}

// TODO: bat_angle_deg must be added to BatterInfo
// TODO: bat_angle_deg must be added to BatterInfo
pub fn evaluate_swing_contact(
    batter: &BatterInfo,
    spacial_offset: &PitchDisplacement,
    timing_offset_sec: f64,
    swing_execution_error: &SwingExecutionError,
) -> SwingContactResult {
    // Timing delay (seconds) × bat swing speed (m/s) = X-axis impact position shift due to timing delay (m)
    let timing_impact_x_m = batter.swing_speed * timing_offset_sec;
    let offset_x_m = spacial_offset.horizontal_offset_m
        + swing_execution_error.additional_x_m
        + timing_impact_x_m;
    let offset_z_m = spacial_offset.vertical_offset_m + swing_execution_error.additional_z_m;

    let rad = swing_execution_error.actual_bat_angle_deg.to_radians();

    // Offset projected onto the bat's thickness direction (effective) using bat angle (bat_angle_deg)
    // Project X/Z spatial offsets onto the bat's thickness direction
    let thickness_offset_m = (-offset_x_m * rad.sin() + offset_z_m * rad.cos()).abs();

    // Offset projected onto the bat's length direction (m) using bat angle (bat_angle_deg)
    // Project X/Z spatial offsets onto the bat's length direction
    let length_offset_m = (offset_x_m * rad.cos() + offset_z_m * rad.sin()).abs();

    // 1. Bat length limit (e.g. miss if more than 35cm from the sweet spot toward the end)
    let contact_type = if length_offset_m > 0.350 {
        SwingContactType::SwungAndMiss

    // 2. Bat thickness direction (bat radius 3.3cm + ball radius 3.7cm = 7.0cm limit)
    } else if thickness_offset_m > 0.070 {
        SwingContactType::SwungAndMiss
    } else if thickness_offset_m > 0.055 {
        SwingContactType::FoulTip
    } else if thickness_offset_m > 0.025 {
        SwingContactType::WeakContact
    } else {
        SwingContactType::SolidContact
    };

    SwingContactResult {
        offset_x_m: offset_x_m,
        offset_z_m: offset_z_m,
        thickness_offset_m: thickness_offset_m,
        length_offset_m: length_offset_m,
        timing_offset: timing_offset_sec,
        contact_type: contact_type,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BattedBallAngles {
    pub vla_deg: f64, // Vertical launch angle (deg): + upward pop / - grounder
    pub hla_deg: f64, // Horizontal launch angle (deg): - pull / + opposite (right-handed batter reference)
}

pub fn calculate_launch_angles(
    contact_result: &SwingContactResult,
    attack_angle_deg: f64,
    batting_side: RL,
) -> BattedBallAngles {
    // Constant definitions
    const EFFECTIVE_RADIUS_M: f64 = 0.070; // Bat radius (3.3cm) + ball radius (3.7cm)
    const SWING_ARM_RADIUS_M: f64 = 1.10; // Swing rotation radius (1.1m)

    // 1. Calculate VLA (vertical launch angle)
    // Clamp z_m to the effective radius and compute arcsin
    let clamped_z = contact_result
        .offset_z_m
        .clamp(-EFFECTIVE_RADIUS_M, EFFECTIVE_RADIUS_M);
    let normal_angle_z_rad = (clamped_z / EFFECTIVE_RADIUS_M).asin();

    const VLA_REBOUND_FACTOR: f64 = 0.60; // Contact surface deflection influence
    let vla_deg = attack_angle_deg + (normal_angle_z_rad.to_degrees() * VLA_REBOUND_FACTOR);

    // 2. Calculate HLA (horizontal launch angle)
    // (A) Bat face tilt from swing rotation (Face Angle)
    let clamped_x_arm = contact_result
        .offset_x_m
        .clamp(-SWING_ARM_RADIUS_M, SWING_ARM_RADIUS_M);
    let face_angle_rad = (clamped_x_arm / SWING_ARM_RADIUS_M).asin();

    // (B) Rebound deflection from the bat's cross-section curvature
    let clamped_x_rad = contact_result
        .offset_x_m
        .clamp(-EFFECTIVE_RADIUS_M, EFFECTIVE_RADIUS_M);
    let rebound_angle_x_rad = (clamped_x_rad / EFFECTIVE_RADIUS_M).asin();

    const HLA_FACE_FACTOR: f64 = 0.85;
    const HLA_REBOUND_FACTOR: f64 = 0.25;

    let raw_hla_deg = (face_angle_rad.to_degrees() * HLA_FACE_FACTOR)
        + (rebound_angle_x_rad.to_degrees() * HLA_REBOUND_FACTOR);

    // Flip the pull/opposite sign for left-handed batters
    let hla_deg = if batting_side == RL::Right {
        raw_hla_deg
    } else {
        -raw_hla_deg
    };

    BattedBallAngles { vla_deg, hla_deg }
}

pub fn calculate_launch_speed(
    contact_result: &SwingContactResult,
    ball_speed: f64,
    swing_speed: f64,
) -> f64 {
    // 1. Theoretical maximum exit velocity on perfect sweet-spot contact (m/s)
    const C_PITCH: f64 = 0.18; // Pitch speed contribution (18%)
    const C_SWING: f64 = 1.20; // Swing speed contribution (120%)
    let max_launch_speed = (C_PITCH * ball_speed) + (C_SWING * swing_speed);

    // 2. Thickness-direction energy decay rate (E_thick: 0.0 ~ 1.0)
    const SWEET_SPOT_RADIUS_M: f64 = 0.020; // Sweet spot radius (2.0cm)
    const MAX_CONTACT_RADIUS_M: f64 = 0.070; // Contact limit (7.0cm)

    let e_thick = if contact_result.thickness_offset_m <= SWEET_SPOT_RADIUS_M {
        1.0
    } else if contact_result.thickness_offset_m >= MAX_CONTACT_RADIUS_M {
        0.0
    } else {
        // Smoothly decay between 2cm ~ 7cm using Smoothstep / Cosine
        let normalized_dist = (contact_result.length_offset_m - SWEET_SPOT_RADIUS_M)
            / (MAX_CONTACT_RADIUS_M - SWEET_SPOT_RADIUS_M);
        (normalized_dist * std::f64::consts::FRAC_PI_2)
            .cos()
            .powi(2)
    };

    // 4. Length-direction energy decay rate (E_len: 0.0 ~ 1.0)
    // Decays as distance from the sweet spot approaches 35cm (handle/tip)
    const MAX_LENGTH_OFFSET_M: f64 = 0.35;
    let e_len =
        (1.0 - (contact_result.length_offset_m / MAX_LENGTH_OFFSET_M).powi(2)).clamp(0.0, 1.0);

    // 5. Calculate final batted ball exit velocity
    let launch_speed_ms = max_launch_speed * e_thick * e_len;

    launch_speed_ms
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, EnumIter, AsRefStr,
)]
#[strum(ascii_case_insensitive)]
pub enum FieldSector {
    FoulPull, // NOTE: Foul of Pull-side (right-handed batter → left field, left-handed batter → right field)
    Pull,     // NOTE: Pull (right-handed batter → left field, left-handed batter → right field)
    Center,   // NOTE: Center field
    Opposite, // NOTE: Opposite field (right-handed batter → right field, left-handed batter → left field)
    FoulOpposite, // NOTE: Foul of Opposite-side (right-handed batter → right field, left-handed batter → left field)
}

#[derive(Clone, Debug)]
pub struct SpinVector {
    pub x: f64, // Horizontal spin component (+: slider spin, -: screw spin)
    pub y: f64, // Vertical spin component (+: backspin, -: topspin)
}
impl SpinVector {
    /// Create a spin vector from spin_rate and spin_angle (deg)
    pub fn from_polar(rate: f64, angle_deg: f64) -> Self {
        let angle_rad = angle_deg.to_radians();
        Self {
            x: rate * angle_rad.sin(),
            y: rate * angle_rad.cos(),
        }
    }

    /// Convert from vector back to (spin_rate, spin_angle_deg)
    pub fn to_polar(&self) -> (f64, f64) {
        let rate = (self.x.powi(2) + self.y.powi(2)).sqrt();

        // atan2(x, y) gives angle where 12 o'clock (positive Y) is 0 degrees
        let mut angle_deg = self.x.atan2(self.y).to_degrees();
        if angle_deg < 0.0 {
            angle_deg += 360.0;
        }

        (rate, angle_deg)
    }
}

/// Combine collision spin and pitch spin to calculate the final batted ball spin
fn combine_batted_spin(
    collision_spin_rate: f64,
    collision_spin_angle: f64,
    pitch_spin_rate: f64,
    pitch_spin_angle: f64,
) -> (f64, f64) {
    // 1. Convert collision spin to vector
    let collision_vec = SpinVector::from_polar(collision_spin_rate, collision_spin_angle);

    // 2. Convert residual pitch spin to vector
    // Note: proportion of pitch spin transferred to batted ball at contact (approx. 15%~25%)
    let retention_rate = 0.20;
    let pitch_retained_rate = pitch_spin_rate * retention_rate;

    // Pitch spin vector
    let pitch_vec = SpinVector::from_polar(pitch_retained_rate, pitch_spin_angle);

    // 3. Simple vector addition (component-wise)
    let total_vec = SpinVector {
        x: collision_vec.x + pitch_vec.x,
        y: collision_vec.y + pitch_vec.y,
    };

    // 4. Convert vector back to final spin_rate and spin_angle
    let (final_rate, final_angle) = total_vec.to_polar();

    (final_rate, final_angle)
}

fn calculate_collision_spin(
    ball: PitchedBall,
    swing_speed: f64,
    contact: &SwingContactResult,
) -> (f64, f64) {
    // 1. Calculate total distance from sweet spot using Pythagorean theorem
    let distance = (contact.length_offset_m.powi(2) + contact.thickness_offset_m.powi(2))
        .sqrt()
        .min(1.0);

    // 2. Calculate contact point angle (radians) using atan2(y, x)
    // y = vertical, x = horizontal
    let impact_angle_rad = contact.thickness_offset_m.atan2(contact.length_offset_m);

    // 3. Convert contact angle to spin angle
    // The ball spins in the opposite direction from the contact point (+ PI)
    let spin_angle_rad = impact_angle_rad + PI;

    // Convert from mathematical polar coordinates (East=0°, North=90°) to
    // baseball spin notation (12 o'clock/North=0°=backspin, 3 o'clock/East=90°=slider spin)
    // Note: hitting the top of the ball (North, Y=+1, X=0) => impact_rad = PI/2 => spin_rad = 3/2 PI => 180°(topspin)
    let mut spin_angle_deg = 90.0 - spin_angle_rad.to_degrees();

    // Normalize to 0.0~360.0 degrees
    if spin_angle_deg < 0.0 {
        spin_angle_deg += 360.0;
    }

    // 4. Calculate spin rate
    let swing_power_factor = swing_speed / REF_SWING_SPEED;
    let max_possible_spin = MAX_COLLISION_SPIN_AT_REF_SPEED * swing_power_factor;
    let raw_spin_rate = max_possible_spin * distance;

    // 5. Combine with pitch spin
    let (combined_spin_rate, combined_spin_angle) = combine_batted_spin(
        ball.spin_rate,
        ball.spin_angle,
        raw_spin_rate,
        spin_angle_deg,
    );

    (combined_spin_rate, combined_spin_angle)
}

/// Determine final batted ball category by combining launch angle and spin
fn classify_trajectory_type(launch_angle: f64, spin_rate: f64, spin_angle: f64) -> TrajectoryType {
    // 1. Calculate trajectory correction from spin (lift/sink)
    // Backspin (0°) gives positive correction, topspin (180°) gives negative correction
    let spin_angle_rad = spin_angle.to_radians();
    let backspin_factor = spin_angle_rad.cos(); // 0 deg => 1.0, 180 deg => -1.0

    // Lift/sink correction proportional to spin rate (approximately ±a few degrees)
    let spin_lift_effect = (spin_rate / 2000.0) * backspin_factor * 3.5;

    // Effective angle incorporating spin effect
    let effective_angle = launch_angle + spin_lift_effect;

    // 2. Category classification (applying spin correction to MLB Statcast standard thresholds)
    if effective_angle < 10.0 {
        TrajectoryType::Grounder
    } else if effective_angle < 25.0 {
        TrajectoryType::Liner
    } else if effective_angle < 50.0 {
        TrajectoryType::Fly
    } else {
        TrajectoryType::PopUp
    }
}

fn calculate_3d_flight_path(
    launch_speed_ms: f64, // Batted ball exit velocity (m/s)
    vla_deg: f64,         // Vertical launch angle VLA (deg)
    hla_deg: f64,         // Horizontal launch angle HLA (deg) (+: right/opposite, -: left/pull)
    spin_rate: f64,       // Spin rate (rpm)
    spin_angle_deg: f64,  // Spin angle (deg) (0: backspin, 90: slider spin, 270: screw spin)
) -> (f64, f64, f64) {
    let vla_rad = vla_deg.to_radians();
    let hla_rad = hla_deg.to_radians();
    let spin_angle_rad = spin_angle_deg.to_radians();

    // 1. Decompose Magnus acceleration (vertical vs horizontal)
    // Total Magnus force (approx. 3.5 m/s² at 2500rpm, 40m/s)
    let total_magnus_accel = (spin_rate / 2500.0) * (launch_speed_ms / 40.0) * 3.5;

    // 2. Decompose into vertical lift (cos) and horizontal break (sin)
    let vertical_magnus = total_magnus_accel * spin_angle_rad.cos();
    let side_magnus = total_magnus_accel * spin_angle_rad.sin();

    // Horizontal initial velocity component
    let v_vertical = launch_speed_ms * vla_rad.sin();
    let v_horizontal = launch_speed_ms * vla_rad.cos().max(0.0);

    // 3. Calculate effective gravity (gravity 9.81 - Magnus lift)
    let g_eff = (GRAVITY - vertical_magnus).max(2.0);

    // 4. Contact height (impact position above ground: e.g. 0.9m)
    const IMPACT_HEIGHT_M: f64 = 0.90;

    // 5. Hang time (solution of quadratic: z0 + vz*t - 0.5*g*t^2 = 0)
    // Even with negative VLA (grounder), sqrt(vz² + 2*g*z0) is larger than vz, so the positive solution is always valid
    let discriminant = v_vertical.powi(2) + (2.0 * g_eff * IMPACT_HEIGHT_M);
    let flight_time_sec = (v_vertical + discriminant.sqrt()) / g_eff;

    // 6. Air resistance correction (simplified model)
    let drag_factor = (1.0 - (0.005 * launch_speed_ms) - (0.0001 * spin_rate)).clamp(0.5, 0.95);

    // 7. Y-axis flight distance (linear distance × cos(HLA) × air resistance)
    let distance = (v_horizontal * hla_rad.cos().max(0.0) * flight_time_sec) * drag_factor;

    // 3. Calculate horizontal arrival angle
    // A. Horizontal break from side spin during flight (1/2 * a * t²)
    let x_from_side_spin = 0.5 * side_magnus * flight_time_sec.powi(2);

    // B. Add side-spin deflection angle to initial launch angle (hla_deg)
    // atan2(lateral deviation, depth distance) gives additional deflection angle (rad)
    let spin_curve_angle_rad = x_from_side_spin.atan2(distance);
    let spin_curve_angle_deg = spin_curve_angle_rad.to_degrees();

    // Final horizontal arrival angle (polar coordinate angle θ)
    let final_spray_angle_deg = hla_deg + spin_curve_angle_deg;

    (flight_time_sec, distance, final_spray_angle_deg)
}

pub fn calculate_batted_ball(
    batter: &BatterInfo,
    ball: PitchedBall,
    contact: &SwingContactResult,
) -> BattedBall {
    // 1. Calculate exit velocity (damped by spatial offset & timing delay)
    let launch_speed_ms = calculate_launch_speed(contact, ball.speed, batter.swing_speed);

    // 2. Calculate vertical launch angle (VLA) and horizontal launch angle (HLA)
    let angles = calculate_launch_angles(&contact, batter.attack_angle, batter.batting_side);

    // 3. Calculate batted ball spin
    // Inherit a small portion of the residual spin from pitch.spin_rate / pitch.spin_angle
    let (batted_spin_rate, batted_spin_angle) =
        calculate_collision_spin(ball, batter.swing_speed, contact);
    let trajectory = classify_trajectory_type(angles.vla_deg, batted_spin_rate, batted_spin_angle);

    let (hang_time, distance, spray_angle) = calculate_3d_flight_path(
        launch_speed_ms,
        angles.vla_deg,
        angles.hla_deg,
        batted_spin_rate,
        batted_spin_angle,
    );

    BattedBall::new(
        launch_speed_ms * CONVERT_FACTOR_MS_TO_KMH,
        angles.vla_deg,
        spray_angle,
        distance,
        hang_time,
        trajectory,
    )
}

#[cfg(test)]
mod tests {
    use crate::domain::resolver::batting_resolver::{
        BatterInfo, SwingContactResult, SwingContactType, calculate_batted_ball,
        calculate_launch_angles,
    };
    use crate::domain::shared::ball::{BallLocation, PitchedBall, TrajectoryType};
    use crate::domain::shared::player::{PitchType, RL};
    use crate::domain::strategy::pitch_call::TargetZone;
    use crate::domain::util::Vector3D;

    fn batter(batting_side: RL) -> BatterInfo {
        BatterInfo {
            batting_side,
            batting_eye: 0.5,
            swing_speed: 150.0,
            swing_power: 1.0,
            attack_angle: 28.0,
            bat_contact: 0.8,
            timing_bias: 0.0,
            consistency_sigma: 0.03,
        }
    }

    fn assert_between(value: f64, min: f64, max: f64) {
        assert!(
            value >= min && value <= max,
            "{} was outside [{}, {}]",
            value,
            min,
            max
        );
    }

    fn centered_contact() -> SwingContactResult {
        SwingContactResult {
            offset_x_m: 0.0,
            offset_z_m: 0.0,
            thickness_offset_m: 0.0,
            length_offset_m: 0.0,
            timing_offset: 0.0,
            contact_type: SwingContactType::SolidContact,
        }
    }

    #[test]
    fn calculate_launch_angles_uses_contact_offsets_and_batting_side() {
        let cases = [
            (
                SwingContactResult {
                    offset_x_m: 0.07,
                    offset_z_m: 0.07,
                    thickness_offset_m: 0.0,
                    length_offset_m: 0.0,
                    timing_offset: 0.0,
                    contact_type: SwingContactType::SolidContact,
                },
                RL::Right,
                59.0,
                25.6,
            ),
            (
                SwingContactResult {
                    offset_x_m: 0.07,
                    offset_z_m: -0.07,
                    thickness_offset_m: 0.0,
                    length_offset_m: 0.0,
                    timing_offset: 0.0,
                    contact_type: SwingContactType::SolidContact,
                },
                RL::Left,
                -49.0,
                -25.6,
            ),
            (centered_contact(), RL::Right, 5.0, 0.0),
        ];

        for (contact, batting_side, expected_vla, expected_hla) in cases {
            let angles = calculate_launch_angles(&contact, 5.0, batting_side);
            assert!((angles.vla_deg - expected_vla).abs() < 0.1);
            assert!((angles.hla_deg - expected_hla).abs() < 0.1);
        }
    }

    #[test]
    fn calculate_batted_ball_sets_physical_values_and_trajectory_specific_launch_angle() {
        let right_pull_hitter = batter(RL::Right);

        for _ in 0..50 {
            let ball = calculate_batted_ball(
                &right_pull_hitter,
                PitchedBall {
                    pitch_type: PitchType::FourSeamFastball,
                    speed: 150.0,
                    spin_rate: 2300.0,
                    spin_angle: 0.0,
                    spin_efficiency: 0.95,
                    release_point: Vector3D {
                        x: 1.6,
                        y: 16.74,
                        z: 1.7,
                    },
                    flight_time: 0.42,
                    aim_zone: TargetZone::Center,
                    aim_location: BallLocation { x: 0.0, y: 0.0 },
                    actual_location: BallLocation { x: 0.0, y: 0.0 },
                },
                &centered_contact(),
            );

            assert!(ball.launch_speed >= 30.0);
            assert!(ball.distance().is_finite());
            assert!(ball.hang_time.is_finite());
            assert_between(ball.angle(), -1.0, 1.0);

            match ball.trajectory {
                TrajectoryType::Grounder => assert_between(ball.launch_angle, 0.0, 10.0),
                TrajectoryType::Liner => assert_between(ball.launch_angle, 10.0, 25.0),
                TrajectoryType::Fly => assert_between(ball.launch_angle, 25.0, 50.0),
                TrajectoryType::PopUp => assert_between(ball.launch_angle, 50.0, 80.0),
                TrajectoryType::NA => panic!("calculated batted ball should have a trajectory"),
            }
        }
    }
}
