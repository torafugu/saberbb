use crate::domain::random_provider::RandomProvider;
use crate::domain::resolver::pitching_resolver::PitchDisplacement;
use crate::domain::shared::ball::{
    BallLocation, BattedBall, FOUL_DEGREE, MAGNUS_COEFF, OutboundResult, PitchedBall,
};
use crate::domain::shared::game_state::GameError;
use crate::domain::shared::player::{BatterInfo, PitchType, PitcherInfo, RL};
use crate::domain::shared::stadium::Stadium;
use crate::domain::strategy::batting_strategy::{SwingExecution, calculate_attack_angle_modifier};
use crate::domain::strategy::pitching_strategy::{TargetZone, TargetZoneSimilarity};
use crate::domain::util::{GRAVITY, PolarPosition, sigmoid};

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use strum_macros::{AsRefStr, EnumString};

// Standard reference swing speed (km/h)
const REF_SWING_SPEED: f64 = 120.0;
// Maximum spin rate generated when fully brushing the ball at reference swing (rpm)
const MAX_COLLISION_SPIN_AT_REF_SPEED: f64 = 4000.0;

pub struct BattingFactor {
    pub zone_similarity: f64,
    pub pitch_similarity: f64,
    pub zone_aptitude: f64,
    pub total_modifier: f64,
}

pub fn calculate_batting_factor(
    pitcher: &PitcherInfo,
    batter: &BatterInfo,
    actual_pitch_type: PitchType,
    expected_pitch_type: PitchType,
    actual_location: &BallLocation,
    expected_location: &BallLocation,
) -> BattingFactor {
    let zone_similarity = calculate_zone_similarity_factor(actual_location, expected_location);
    let pitch_similarity =
        calculate_pitch_similarity(pitcher, actual_pitch_type, expected_pitch_type);
    let zone_aptitude = batter.zone_modifier(actual_location);
    let total_modifier =
        ((1.0 - zone_aptitude) + (1.0 - zone_similarity) + (1.0 - pitch_similarity)) / 3.0;

    BattingFactor {
        zone_similarity,
        pitch_similarity,
        zone_aptitude,
        total_modifier,
    }
}

fn calculate_zone_similarity_factor(
    actual_location: &BallLocation,
    expected_location: &BallLocation,
) -> f64 {
    let actual_target_zone = actual_location.target_zone();
    let expected_target_zone = expected_location.target_zone();
    if actual_target_zone == TargetZone::Center && expected_target_zone == TargetZone::Center {
        0.3
    } else if actual_target_zone == TargetZone::Center && expected_target_zone != TargetZone::Center
    {
        0.1
    } else {
        match actual_target_zone.similarity(expected_target_zone) {
            TargetZoneSimilarity::Same => 0.2,
            TargetZoneSimilarity::Opposite => -0.2,
            _ => 0.05,
        }
    }
}

fn calculate_pitch_similarity(
    pitcher: &PitcherInfo,
    actual_pitch_type: PitchType,
    expected_pitch_type: PitchType,
) -> f64 {
    let actual_pitch_skill = pitcher.select_pitch_skill(actual_pitch_type);
    let expected_pitch_skill = pitcher.select_pitch_skill(expected_pitch_type);

    let spin_rate_similarity =
        0.2 - (actual_pitch_skill.spin_rate - expected_pitch_skill.spin_rate).abs();
    let spin_angle_similarity =
        0.2 - ((actual_pitch_skill.spin_angle - expected_pitch_skill.spin_angle) / 360.0).abs();

    (spin_rate_similarity + spin_angle_similarity).clamp(-0.2, 0.2)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, AsRefStr)]
pub enum CountStatus {
    C00,
    C10,
    C20,
    C30,
    C01,
    C11,
    C21,
    C31,
    C02,
    C12,
    C22,
    C32,
}
impl CountStatus {
    pub fn prob(&self) -> f64 {
        match self {
            CountStatus::C00 => 0.0,
            CountStatus::C10 => -0.10,
            CountStatus::C20 => -0.25,
            CountStatus::C30 => -0.6,
            CountStatus::C01 => 0.1,
            CountStatus::C11 => 0.0,
            CountStatus::C21 => -0.1,
            CountStatus::C31 => -0.25,
            CountStatus::C02 => 0.2,
            CountStatus::C12 => 0.1,
            CountStatus::C22 => 0.05,
            CountStatus::C32 => 0.15,
        }
    }

    pub fn is_strike_two(&self) -> bool {
        match self {
            CountStatus::C02 | CountStatus::C12 | CountStatus::C22 | CountStatus::C32 => true,
            _ => false,
        }
    }
}

#[derive(Clone)]
pub enum PlateApproach {
    Aggressive,
    Patient,
    Take,
}
impl PlateApproach {
    pub fn prob(&self) -> f64 {
        match self {
            PlateApproach::Aggressive => 0.2,
            PlateApproach::Patient => -0.1,
            PlateApproach::Take => -5.0,
        }
    }
}

pub fn calculate_swing_factor(
    approach: PlateApproach,
    count_status: CountStatus,
    pitch_type: PitchType,
    batting_factor: &BattingFactor,
) -> f64 {
    let mut count_status_factor = count_status.prob();

    if !count_status.is_strike_two() {
        count_status_factor += approach.prob();
    }

    let fastball_factor = if pitch_type == PitchType::FourSeamFastball {
        0.1
    } else {
        -0.05
    };

    count_status_factor
        + fastball_factor
        + batting_factor.zone_similarity
        + batting_factor.pitch_similarity
        + batting_factor.zone_aptitude
}

pub fn select_swing_execution(
    rng: &mut dyn RandomProvider,
    swing_execution_factor: f64,
) -> SwingExecution {
    if rng.random() < sigmoid(swing_execution_factor) {
        SwingExecution::Swing
    } else {
        SwingExecution::Take
    }
}

pub fn adapt_to_pitch(
    offset: &PitchDisplacement,
    bat_control: f64,
    batting_factor: &BattingFactor,
) -> PitchDisplacement {
    let total_modifier =
        ((sigmoid(bat_control) + batting_factor.total_modifier) / 2.0).clamp(0.1, 1.2);
    // 1. Absorb spatial offset with contact skill (e.g. reduce 0.10m offset to 0.05m)
    let adapted_x = offset.horizontal_offset_m * total_modifier;
    let adapted_z = offset.vertical_offset_m * total_modifier;

    // 2. Adjust timing offset with bat lag/steering (e.g. shrink 0.012s delay to 0.006s)
    let adapted_timing = offset.timing_offset_sec * total_modifier;

    PitchDisplacement {
        crossfire_multiplier: offset.crossfire_multiplier,
        release_x_factor: offset.release_x_factor,
        horizontal_offset_m: adapted_x,
        vertical_offset_m: adapted_z,
        timing_offset_sec: adapted_timing,
    }
}

fn calculate_bat_angle(location: &BallLocation) -> f64 {
    const CENTER_ANGLE_DEG: f64 = 30.0; // Standard tilt angle at zone center
    const HIGH_LOW_RANGE_DEG: f64 = 15.0; // Angle variation range based on height

    // Map norm_y (+1.0 high ~ -1.0 low) to 15° ~ 45°
    let base_angle = CENTER_ANGLE_DEG - (location.y * HIGH_LOW_RANGE_DEG);

    // Clamp to human range of motion limits (10° ~ 60°)
    base_angle.clamp(10.0, 60.0)
}

/// Automatically calculate the effective Attack Angle linked to bat_angle_deg (head drop amount for pitch location)
fn calculate_dynamic_attack_angle(attack_angle_deg: f64, bat_angle_deg: f64) -> f64 {
    // Baseline bat_angle_deg (e.g. 30° as the standard tilt)
    const BASE_BAT_ANGLE_DEG: f64 = 30.0;

    // Attack angle rises approx. 3.5° for every 10° deeper bat_angle
    const COUPLING_FACTOR: f64 = 0.35;

    let angle_delta = bat_angle_deg - BASE_BAT_ANGLE_DEG;

    // Determine the dynamic attack angle
    attack_angle_deg + (angle_delta * COUPLING_FACTOR)
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct SwingExecutionError {
    pub additional_x_m: f64,
    pub additional_z_m: f64,
    pub ideal_bat_angle_deg: f64,
    pub actual_bat_angle_deg: f64,
    pub ideal_attack_angle_deg: f64,
    pub actual_attack_angle_deg: f64,
}

// Calculate the actual bat_angle_deg the batter swings through based on the real trajectory and their prediction
pub fn calculate_swing_execution_error(
    rng: &mut dyn RandomProvider,
    batter: &BatterInfo,
    actual_location: &BallLocation,
) -> SwingExecutionError {
    let zone_modifier = 1.0 - batter.zone_modifier(actual_location);
    let bat_control_modifier = batter.bat_control / 20.0;

    // 1. Calculate the difference between intended and ideal angle (angle error)
    let intended_location = BallLocation {
        x: actual_location.x + actual_location.x * bat_control_modifier * zone_modifier,
        y: actual_location.y + actual_location.y * bat_control_modifier * zone_modifier,
    };
    let intended_angle = calculate_bat_angle(&intended_location);
    let ideal_angle = calculate_bat_angle(actual_location);

    // Lower contact skill leaves a larger Δθ because the swing can't correct toward the ideal angle
    let unadjusted_ratio = (1.0 - sigmoid(batter.bat_control)).clamp(0.1, 1.0);
    let delta_angle_deg = (intended_angle - ideal_angle) * unadjusted_ratio;

    // 2. Convert angle error (deg) to spatial meter error (Δx, Δz)
    let delta_rad = delta_angle_deg.to_radians();
    const BAT_BARREL_LENGTH_M: f64 = 0.70; // Distance from grip to sweet spot

    let additional_z_m = BAT_BARREL_LENGTH_M * delta_rad.sin();
    let additional_x_m = BAT_BARREL_LENGTH_M * (1.0 - delta_rad.cos());

    // 3. Calculate actual bat angle and attack angle the batter swung
    let actual_bat_angle_deg = intended_angle - (intended_angle - ideal_angle);
    let actual_attack_angle_deg =
        calculate_dynamic_attack_angle(batter.attack_angle, actual_bat_angle_deg)
            + calculate_attack_angle_modifier(batter.batter_type)
                * rng.normal_factor_std_5_percent();

    SwingExecutionError {
        additional_x_m,
        additional_z_m,
        ideal_bat_angle_deg: ideal_angle,
        actual_bat_angle_deg,
        ideal_attack_angle_deg: batter.attack_angle,
        actual_attack_angle_deg,
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
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SwingContactResult {
    pub timing_impact_x_m: f64,
    pub offset_x_m: f64,
    pub offset_z_m: f64,
    // NOTE: Spatial sweet-spot offset (0.0: perfectly centered ~ 1.0: completely missing the zone)
    pub thickness_offset_m: f64,
    pub length_offset_m: f64,
    pub contact_type: SwingContactType,
    pub attack_angle_deg: f64,
}

#[derive(
    Clone, Debug, Default, Copy, PartialEq, Eq, EnumString, Serialize, Deserialize, AsRefStr,
)]
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
    #[default]
    Take,
}

pub fn evaluate_swing_contact(
    batter: &BatterInfo,
    offset: &PitchDisplacement,
    swing_execution_error: &SwingExecutionError,
) -> SwingContactResult {
    // Timing delay (seconds) × bat swing speed (m/s) = X-axis impact position shift due to timing delay (m)
    let timing_impact_x_m = batter.swing_speed * offset.timing_offset_sec;
    let offset_x_m =
        offset.horizontal_offset_m + swing_execution_error.additional_x_m + timing_impact_x_m;
    let offset_z_m = offset.vertical_offset_m + swing_execution_error.additional_z_m;

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
        timing_impact_x_m: timing_impact_x_m,
        offset_x_m: offset_x_m,
        offset_z_m: offset_z_m,
        thickness_offset_m: thickness_offset_m,
        length_offset_m: length_offset_m,
        contact_type: contact_type,
        attack_angle_deg: swing_execution_error.actual_attack_angle_deg,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BattedBallAngles {
    pub vla_deg: f64, // Vertical launch angle (deg): + upward pop / - grounder
    pub hla_deg: f64, // Horizontal launch angle (deg): - pull / + opposite (right-handed batter reference)
}

pub fn calculate_launch_angles(contact: &SwingContactResult, batting_side: RL) -> BattedBallAngles {
    // Constant definitions
    const EFFECTIVE_RADIUS_M: f64 = 0.070; // Bat radius (3.3cm) + ball radius (3.7cm)
    const SWING_ARM_RADIUS_M: f64 = 1.10; // Swing rotation radius (1.1m)

    // 1. Calculate VLA (vertical launch angle)
    // Clamp z_m to the effective radius and compute arcsin
    let clamped_z = contact
        .offset_z_m
        .clamp(-EFFECTIVE_RADIUS_M, EFFECTIVE_RADIUS_M);
    let normal_angle_z_rad = (clamped_z / EFFECTIVE_RADIUS_M).asin();

    const VLA_REBOUND_FACTOR: f64 = 0.60; // Contact surface deflection influence
    let vla_deg = contact.attack_angle_deg + (normal_angle_z_rad.to_degrees() * VLA_REBOUND_FACTOR);

    // 2. Calculate HLA (horizontal launch angle)
    // (A) Bat face tilt from swing rotation (Face Angle)
    let clamped_x_arm = contact
        .offset_x_m
        .clamp(-SWING_ARM_RADIUS_M, SWING_ARM_RADIUS_M);
    let face_angle_rad = (clamped_x_arm / SWING_ARM_RADIUS_M).asin();

    // (B) Rebound deflection from the bat's cross-section curvature
    let clamped_x_rad = contact
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

fn calculate_effective_c_swing(
    base_c_swing: f64,      // Batter's inherent C_SWING (e.g. 1.20)
    timing_impact_x_m: f64, // Contact point depth (m): <0 front (pull) / >0 back (opposite)
) -> f64 {
    // Contacting in front (x_m < 0): gains up to +5% power
    // Contacting deep (x_m > 0): loses up to -15% energy
    if timing_impact_x_m < 0.0 {
        // Contact point in front (transfer rate increases up to -0.20m limit)
        let boost = (timing_impact_x_m.abs() / 0.20).clamp(0.0, 1.0) * 0.05;
        base_c_swing * (1.0 + boost)
    } else {
        // Contact point driven deep (significant reduction at 0.20m delay)
        let penalty = (timing_impact_x_m / 0.20).clamp(0.0, 1.0) * 0.15;
        base_c_swing * (1.0 - penalty)
    }
}

pub fn calculate_launch_speed_with_power(
    contact_result: &SwingContactResult,
    ball_speed: f64,
    swing_speed: f64,
    swing_power: f64,
) -> f64 {
    // 1. Theoretical maximum exit velocity on perfect sweet-spot contact (m/s)
    const C_PITCH: f64 = 0.18; // Pitch speed contribution (18%)
    // Dynamic C_SWING variation based on swing_power (1.12 ~ 1.28)
    let power = calculate_effective_c_swing(sigmoid(swing_power), contact_result.timing_impact_x_m);
    let c_swing: f64 = 1.12 + (0.16 * power);
    let max_launch_speed = (C_PITCH * ball_speed) + (c_swing * swing_speed);

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

// NOTE: Pinpoint info at the moment the ball reaches a specified point
#[derive(Debug, Clone, Copy)]
pub struct TargetArrivalState {
    pub time_sec: f64,        // Time to reach the specified Y distance (s)
    pub distance_m: f64,      // Polar r: distance reached from home (m)
    pub spray_angle_deg: f64, // Polar θ: ball direction angle at that point (deg)
    pub z_m: f64,             // Ball height at that moment (m) -> used for catch judgment
    pub v_z: f64,             // Vertical velocity (to determine falling or rising)
}

pub struct TrajectorySummaryResult {
    pub total_time_sec: f64,
    pub result_type: OutboundResult,

    // Polar coordinates (r, θ) of the final resting position
    pub final_distance_m: f64,
    pub final_spray_angle_deg: f64,

    // Polar coordinates (r) and time (t) of the first bounce
    // None if it hits the fence directly or is a no-bounce home run
    pub first_bounce_distance_m: Option<f64>,
    pub first_bounce_time_sec: Option<f64>,

    // NOTE: Only holds the passage state at that point when target_distance_y is specified
    pub target_arrival: Option<TargetArrivalState>,
}

fn calculate_trajectory(
    launch_speed_ms: f64, // Batted ball exit velocity (m/s)
    vla_deg: f64,         // Vertical launch angle VLA (deg)
    hla_deg: f64,         // Horizontal launch angle HLA (deg) (+: right/opposite, -: left/pull)
    spin_rate_rpm: f64,   // Spin rate (rpm)
    spin_angle_deg: f64,  // Spin angle (deg) (0: backspin, 90: slider spin, 270: screw spin)
    stadium: &Stadium,
) -> Result<BattedBall, GameError> {
    let vla_rad = vla_deg.to_radians();
    let hla_rad = hla_deg.to_radians();

    // Initial velocity vector
    let mut v_x = launch_speed_ms * vla_rad.cos() * hla_rad.sin();
    let mut v_y = launch_speed_ms * vla_rad.cos() * hla_rad.cos().abs();
    let mut v_z = launch_speed_ms * vla_rad.sin();

    // Contact height (impact position above ground: e.g. 0.9m)
    const IMPACT_HEIGHT_M: f64 = 0.90;

    // Initial position
    let mut pos_x: f64 = 0.0;
    let mut pos_y: f64 = 0.0;
    let mut pos_z = IMPACT_HEIGHT_M;
    let mut max_height_m = pos_z; // Tracks the max height reached (initial value: impact height)

    // Current spin state for calculation (decays and changes after bounces)
    let mut current_spin_rate = spin_rate_rpm;
    let mut current_spin_angle = spin_angle_deg;

    // Decompose the wind vector
    let wind = stadium.default_wind();
    let wind_rad = wind.dir_deg.to_radians();
    let wind_v_x = wind.speed_m_per_s * wind_rad.sin();
    let wind_v_y = wind.speed_m_per_s * wind_rad.cos();

    let mut current_time = 0.0;
    let dt = 0.01; // 10ms steps
    let mut bounce_count = 0;

    let mut first_bounce_position = None;
    let mut first_bounce_time_sec = None;
    let mut fence_impact_position = None;
    let mut fence_impact_time_sec = None;
    let mut result_type = OutboundResult::InField;

    const AIR_RESISTANCE_COEFF: f64 = 0.0012; // Simplified air resistance coefficient
    const RESTITUTION_COEFF: f64 = 0.45; // Ground restitution coefficient
    const WALL_RESTITUTION: f64 = 0.60; // Fence restitution coefficient
    const GROUND_FRICTION: f64 = 0.25; // Ground friction coefficient

    loop {
        let prev_x = pos_x;
        let prev_y = pos_y;

        // --- Physics update (air resistance, Magnus, velocity, position) ---
        let rel_v_x = v_x - wind_v_x;
        let rel_v_y = v_y - wind_v_y;
        let rel_v_z = v_z;
        let rel_speed = (rel_v_x.powi(2) + rel_v_y.powi(2) + rel_v_z.powi(2)).sqrt();

        let drag_a_x = -AIR_RESISTANCE_COEFF * rel_speed * rel_v_x;
        let drag_a_y = -AIR_RESISTANCE_COEFF * rel_speed * rel_v_y;
        let drag_a_z = -AIR_RESISTANCE_COEFF * rel_speed * rel_v_z;

        let (mag_a_x, mag_a_y, mag_a_z) = if rel_speed > 0.1 && current_spin_rate > 10.0 {
            // Unit vector of the direction of travel
            let u_x = rel_v_x / rel_speed;
            let u_y = rel_v_y / rel_speed;
            let u_z = rel_v_z / rel_speed;

            // Construct an "upward" reference vector relative to the direction of travel
            let proj_z = u_z;
            let raw_up_x = -proj_z * u_x;
            let raw_up_y = -proj_z * u_y;
            let raw_up_z = 1.0 - proj_z * u_z;
            let up_len = (raw_up_x.powi(2) + raw_up_y.powi(2) + raw_up_z.powi(2)).sqrt();

            let (up_x, up_y, up_z) = if up_len > 0.001 {
                (raw_up_x / up_len, raw_up_y / up_len, raw_up_z / up_len)
            } else {
                (0.0, 0.0, 1.0)
            };

            // "Rightward" vector relative to direction of travel (cross product: Velocity x Up)
            let right_x = u_y * up_z - u_z * up_y;
            let right_y = u_z * up_x - u_x * up_z;
            let right_z = u_x * up_y - u_y * up_x;

            // Resultant direction vector based on spin axis angle
            let spin_rad = current_spin_angle.to_radians();
            let cos_s = spin_rad.cos(); // 0° = backspin (+Up)
            let sin_s = spin_rad.sin(); // +90° = slice (+Right)

            let mag_dir_x = cos_s * up_x + sin_s * right_x;
            let mag_dir_y = cos_s * up_y + sin_s * right_y;
            let mag_dir_z = cos_s * up_z + sin_s * right_z;

            // Magnitude of the Magnus acceleration
            let magnus_accel = MAGNUS_COEFF * rel_speed * current_spin_rate;

            (
                magnus_accel * mag_dir_x,
                magnus_accel * mag_dir_y,
                magnus_accel * mag_dir_z,
            )
        } else {
            (0.0, 0.0, 0.0)
        };

        // Update velocity and position (gravity + air resistance + Magnus effect)
        v_x += (drag_a_x + mag_a_x) * dt;
        v_y += (drag_a_y + mag_a_y) * dt;
        v_z += (-GRAVITY + drag_a_z + mag_a_z) * dt;

        pos_x += v_x * dt;
        pos_y += v_y * dt;
        pos_z += v_z * dt;
        current_time += dt;

        // Update the max height on every step until the first bounce occurs
        if bounce_count == 0 {
            max_height_m = max_height_m.max(pos_z);
        }

        let current_spray_angle_deg = pos_x.atan2(pos_y).to_degrees();
        let current_distance_m = (pos_x.powi(2) + pos_y.powi(2)).sqrt();

        let fence_distace = stadium.fence_distance_at_angle(current_spray_angle_deg)?;

        // 2. Fence clearing / impact judgment
        if current_distance_m >= fence_distace {
            if pos_z > stadium.fence_height {
                result_type = if bounce_count == 0 {
                    if current_spray_angle_deg.abs() >= FOUL_DEGREE {
                        OutboundResult::Foul
                    } else {
                        OutboundResult::HomeRun
                    }
                } else {
                    OutboundResult::GroundRuleDouble
                };
                break;
            } else if pos_z > 0.0 {
                if fence_impact_position.is_none() && fence_impact_time_sec.is_none() {
                    fence_impact_position =
                        Some(PolarPosition::new(fence_distace, current_spray_angle_deg));
                    fence_impact_time_sec = Some(current_time);
                }
                // Fence impact: reverse the velocity in polar terms (bounce back toward the field)
                pos_x = prev_x;
                pos_y = prev_y;
                v_x = -v_x * WALL_RESTITUTION;
                v_y = -v_y * WALL_RESTITUTION;
            }
        }

        // 7. Ground bounce processing
        if pos_z <= 0.0 {
            pos_z = 0.0;
            bounce_count += 1;
            if bounce_count == 1 {
                first_bounce_position = Some(PolarPosition::new(
                    current_distance_m,
                    current_spray_angle_deg,
                ));
                first_bounce_time_sec = Some(current_time);
            }

            v_z = -v_z * RESTITUTION_COEFF;
            v_x *= 1.0 - GROUND_FRICTION;
            v_y *= 1.0 - GROUND_FRICTION;

            // NOTE: After the bounce, ground friction turns the spin into topspin (180°) and the spin rate decays
            current_spin_rate *= 0.5;
            current_spin_angle = 180.0;

            if v_z.abs() < 0.5 {
                v_z = 0.0;
                let horiz_speed = (v_x.powi(2) + v_y.powi(2)).sqrt();
                if horiz_speed < 0.2 {
                    break; // Stopped, end the loop
                }
            }
        }

        if current_time > 20.0 {
            return Err(GameError::TimeOut);
        }
    }

    let final_distance_m = (pos_x.powi(2) + pos_y.powi(2)).sqrt();
    let final_spray_angle_deg = pos_x.atan2(pos_y).to_degrees();

    Ok(BattedBall {
        launch_speed: launch_speed_ms,
        launch_angle: vla_deg,
        spin_rate: spin_rate_rpm,
        spin_angle: spin_angle_deg,
        final_position: PolarPosition::new(final_distance_m, final_spray_angle_deg),
        max_height: max_height_m,
        total_time: current_time,
        first_bounce_position: first_bounce_position,
        first_bounce_time: first_bounce_time_sec,
        fence_impact_position: fence_impact_position,
        fence_impact_time: fence_impact_time_sec,
        outbound_result: result_type,
    })
}

pub fn calculate_batted_ball(
    batter: &BatterInfo,
    ball: PitchedBall,
    contact: &SwingContactResult,
    stadium: &Stadium,
) -> Result<BattedBall, GameError> {
    // 1. Calculate exit velocity (damped by spatial offset & timing delay)
    let launch_speed_ms = calculate_launch_speed_with_power(
        contact,
        ball.speed,
        batter.swing_speed,
        batter.swing_power,
    );

    // 2. Calculate vertical launch angle (VLA) and horizontal launch angle (HLA)
    let angles = calculate_launch_angles(&contact, batter.batting_side);

    // 3. Calculate batted ball spin
    // Inherit a small portion of the residual spin from pitch.spin_rate / pitch.spin_angle
    let (batted_spin_rate, batted_spin_angle) =
        calculate_collision_spin(ball, batter.swing_speed, contact);
    // let trajectory = classify_trajectory_type(angles.vla_deg, batted_spin_rate, batted_spin_angle);

    calculate_trajectory(
        launch_speed_ms,
        angles.vla_deg,
        angles.hla_deg,
        batted_spin_rate,
        batted_spin_angle,
        stadium,
    )
}

#[cfg(test)]
mod tests {
    use crate::domain::resolver::batting_resolver::*;
    use crate::domain::shared::ball::{BallLocation, PitchedBall, TrajectoryType};
    use crate::domain::shared::game_state::{GameError, WindCondition};
    use crate::domain::shared::player::{
        ArmSlot, BatterType, FielderInfo, PitchSkill, PitchType, PitcherInfo, PitcherStyle, RL,
        ZoneAptitude,
    };
    use crate::domain::shared::stadium::Stadium;
    use crate::domain::strategy::pitching_strategy::TargetZone;
    use crate::domain::util::Vector3D;

    type TestResult = Result<(), GameError>;

    fn batter(batting_side: RL) -> BatterInfo {
        BatterInfo {
            batting_side,
            batter_type: BatterType::ClassicAnalyst,
            zone_aptitude: ZoneAptitude::Balanced,
            hot_zone_scale: 0.1,
            batting_eye: 0.5,
            swing_speed: 150.0,
            swing_power: 1.0,
            attack_angle: 28.0,
            bat_control: 0.8,
            consistency: 0.03,
        }
    }

    fn pitcher() -> PitcherInfo {
        PitcherInfo {
            height: 1.85,
            extension: 1.8,
            throw_side: RL::Right,
            arm_slot: ArmSlot::ThreeQuarter,
            pitcher_style: PitcherStyle::BalancedPitcher,
            velocity: 150.0,
            spin_rate: 2400.0,
            control: 0.5,
            stamina: 0.5,
            injury_proneness: 0.5,
            clutch: 0.5,
            hpp: 0.5,
            platoon_splitting: 0.5,
            delivery_motion_time: 1.4,
            consistency: 0.03,
            pitch_skills: vec![
                PitchSkill {
                    pitch_type: PitchType::FourSeamFastball,
                    velocity: 150.0,
                    control: 0.5,
                    stamina: 0.5,
                    injury_proneness: 0.5,
                    spin_rate: 2400.0,
                    spin_angle: 180.0,
                    spin_efficiency: 0.95,
                    usage: 0.7,
                },
                PitchSkill {
                    pitch_type: PitchType::Slider,
                    velocity: 135.0,
                    control: 0.5,
                    stamina: 0.5,
                    injury_proneness: 0.5,
                    spin_rate: 2500.0,
                    spin_angle: 90.0,
                    spin_efficiency: 0.85,
                    usage: 0.3,
                },
            ],
            fielder_info: FielderInfo::new_pitcher(),
        }
    }

    fn batting_factor_for_locations(
        actual_location: BallLocation,
        expected_location: BallLocation,
    ) -> BattingFactor {
        calculate_batting_factor(
            &pitcher(),
            &batter(RL::Right),
            PitchType::FourSeamFastball,
            PitchType::FourSeamFastball,
            &actual_location,
            &expected_location,
        )
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

    fn centered_contact(attack_angle_deg: f64) -> SwingContactResult {
        SwingContactResult {
            timing_impact_x_m: 0.0,
            offset_x_m: 0.0,
            offset_z_m: 0.0,
            thickness_offset_m: 0.0,
            length_offset_m: 0.0,
            contact_type: SwingContactType::SolidContact,
            attack_angle_deg,
        }
    }

    fn no_wind() -> WindCondition {
        WindCondition {
            speed_m_per_s: 0.0,
            dir_deg: 0.0,
        }
    }

    #[test]
    fn calculate_launch_angles_uses_contact_offsets_and_batting_side() {
        let cases = [
            (
                SwingContactResult {
                    timing_impact_x_m: 0.0,
                    offset_x_m: 0.07,
                    offset_z_m: 0.07,
                    thickness_offset_m: 0.0,
                    length_offset_m: 0.0,
                    contact_type: SwingContactType::SolidContact,
                    attack_angle_deg: 5.0,
                },
                RL::Right,
                59.0,
                25.6,
            ),
            (
                SwingContactResult {
                    timing_impact_x_m: 0.0,
                    offset_x_m: 0.07,
                    offset_z_m: -0.07,
                    thickness_offset_m: 0.0,
                    length_offset_m: 0.0,
                    contact_type: SwingContactType::SolidContact,
                    attack_angle_deg: 5.0,
                },
                RL::Left,
                -49.0,
                -25.6,
            ),
            (centered_contact(5.0), RL::Right, 5.0, 0.0),
        ];

        for (contact, batting_side, expected_vla, expected_hla) in cases {
            let angles = calculate_launch_angles(&contact, batting_side);
            assert!((angles.vla_deg - expected_vla).abs() < 0.1);
            assert!((angles.hla_deg - expected_hla).abs() < 0.1);
        }
    }

    #[test]
    fn calculate_batting_factor_scores_same_zone_positive() {
        assert_eq!(
            batting_factor_for_locations(
                BallLocation { x: -0.5, y: -0.5 },
                BallLocation { x: -0.5, y: -0.5 },
            )
            .zone_similarity,
            0.2
        );
        assert_eq!(
            batting_factor_for_locations(
                BallLocation { x: 0.0, y: 0.0 },
                BallLocation { x: 0.0, y: 0.0 },
            )
            .zone_similarity,
            0.3
        );
    }

    #[test]
    fn calculate_batting_factor_penalizes_opposite_zones() {
        assert_eq!(
            batting_factor_for_locations(
                BallLocation { x: -0.5, y: -0.5 },
                BallLocation { x: 0.5, y: 0.5 },
            )
            .zone_similarity,
            -0.2
        );
        assert_eq!(
            batting_factor_for_locations(
                BallLocation { x: -0.5, y: 0.5 },
                BallLocation { x: 0.5, y: -0.5 },
            )
            .zone_similarity,
            -0.2
        );
    }

    #[test]
    fn calculate_batting_factor_uses_small_bonus_for_partial_mismatch() {
        assert_eq!(
            batting_factor_for_locations(
                BallLocation { x: -0.5, y: -0.5 },
                BallLocation { x: 0.5, y: -0.5 },
            )
            .zone_similarity,
            0.05
        );
        assert_eq!(
            batting_factor_for_locations(
                BallLocation { x: 0.0, y: 0.0 },
                BallLocation { x: 0.5, y: 0.5 },
            )
            .zone_similarity,
            0.1
        );
    }

    #[test]
    fn calculate_batted_ball_sets_physical_values() -> TestResult {
        let right_pull_hitter = batter(RL::Right);
        let stadium = Stadium::default();

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
                &centered_contact(right_pull_hitter.attack_angle),
                &stadium,
            )?;

            assert!(ball.launch_speed >= 30.0);
            assert!(ball.distance().is_finite());
            assert!(ball.total_time.is_finite());
            assert_between(ball.angle(), -1.0, 1.0);

            match ball.trajectory() {
                TrajectoryType::Grounder => assert_between(ball.launch_angle, 0.0, 10.0),
                TrajectoryType::Liner => assert_between(ball.launch_angle, 10.0, 25.0),
                TrajectoryType::Fly => assert_between(ball.launch_angle, 25.0, 50.0),
                TrajectoryType::PopUp => assert_between(ball.launch_angle, 50.0, 80.0),
                TrajectoryType::NA => panic!("calculated batted ball should have a trajectory"),
            }
        }

        Ok(())
    }
}
