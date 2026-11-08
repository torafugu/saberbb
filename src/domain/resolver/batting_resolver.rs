use crate::domain::random_provider::RandomProvider;
use crate::domain::resolver::pitching_resolver::PitchDisplacement;
use crate::domain::shared::ball::{
    BallLocation, BattedBall, FOUL_DEGREE, MAGNUS_COEFF, OutboundResult, PitchedBall,
};
use crate::domain::shared::game_state::GameError;
use crate::domain::shared::game_stats::PlayerGameBattingMetrics;
use crate::domain::shared::player::{BatterInfo, PitchType, PitcherInfo};
use crate::domain::shared::stadium::Stadium;
use crate::domain::strategy::batting_strategy::SwingExecution;
use crate::domain::strategy::pitching_strategy::{TargetZone, TargetZoneSimilarity};
use crate::domain::util::{GRAVITY, PolarPosition, sigmoid};

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use strum_macros::{AsRefStr, EnumString};

// 16cm from the sweet spot toward the end of the bat
const MAX_LENGTH_OFFSET_M: f64 = 0.16;
/// Trial setting: change to 1.0 for the unscaled control; recorded on every swing.
const CONTACT_TIMING_POSITION_SCALE: f64 = 0.75;
// Standard reference swing speed (m/s)
const REF_SWING_SPEED: f64 = 33.333;
// Maximum spin rate generated when fully brushing the ball at reference swing (rpm)
const MAX_COLLISION_SPIN_AT_REF_SPEED: f64 = 4000.0;

pub struct BattingFactor {
    pub distance_from_zone_edge: f64,
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
    let distance_from_zone_edge = actual_location.distance_from_zone_edge();
    let zone_similarity = calculate_zone_similarity_factor(actual_location, expected_location);
    let pitch_similarity =
        calculate_pitch_similarity(pitcher, actual_pitch_type, expected_pitch_type);
    let zone_aptitude = batter.zone_modifier(actual_location);
    let total_modifier =
        ((1.0 - zone_aptitude) + (1.0 - zone_similarity) + (1.0 - pitch_similarity)) / 3.0;

    BattingFactor {
        distance_from_zone_edge,
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
    match (
        actual_location.target_zone(),
        expected_location.target_zone(),
    ) {
        (Some(TargetZone::Center), Some(TargetZone::Center)) => 0.3,
        (Some(TargetZone::Center), Some(_)) => 0.1,
        (Some(actual_target_zone), Some(expected_target_zone)) => {
            match actual_target_zone.similarity(expected_target_zone) {
                TargetZoneSimilarity::Same => 0.2,
                TargetZoneSimilarity::Opposite => -0.2,
                _ => 0.05,
            }
        }
        _ => -0.2,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, AsRefStr, Serialize, Deserialize)]
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
    calculate_swing_decision(approach, count_status, pitch_type, batting_factor).score
}

/// Logged before updating the count. Coordinates are normalized strike-zone units.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SwingDecisionDiagnostics {
    pub count_status: CountStatus,
    pub location_x: f64,
    pub location_y: f64,
    pub count_factor: f64,
    pub approach_factor: f64,
    pub protection_factor: f64,
    pub zone_factor: f64,
    pub pitch_factor: f64,
    pub anticipation_factor: f64,
    pub aptitude_factor: f64,
    pub score: f64,
    pub probability: f64,
    pub swung: bool,
}

pub fn calculate_swing_decision(
    approach: PlateApproach,
    count_status: CountStatus,
    pitch_type: PitchType,
    batting_factor: &BattingFactor,
) -> SwingDecisionDiagnostics {
    let two_strikes = count_status.is_strike_two();
    let outside = batting_factor.distance_from_zone_edge.max(0.0);
    let count_factor = count_status.prob();
    // Keep aggression; suspend a pre-two-strike take/patient preference when protecting.
    let approach_factor = if two_strikes { approach.prob().max(0.0) } else { approach.prob() };
    // +1.75 log-odds in the zone (about +1.40 over the previous policy).
    // Exponential decay limits the extra incentive to chase outside pitches.
    let protection_factor = if two_strikes { 1.75 * (-12.0 * outside).exp() } else { 0.0 };
    let zone_factor = -2.0 * outside;
    let pitch_factor = if pitch_type == PitchType::FourSeamFastball { 0.1 } else { -0.05 };
    let anticipation = batting_factor.zone_similarity + batting_factor.pitch_similarity;
    let anticipation_factor = if two_strikes && anticipation < 0.0 { anticipation * 0.25 } else { anticipation };
    let aptitude_factor = batting_factor.zone_aptitude;
    let score = count_factor + approach_factor + protection_factor + zone_factor
        + pitch_factor + anticipation_factor + aptitude_factor;
    SwingDecisionDiagnostics {
        count_status, location_x: 0.0, location_y: 0.0, count_factor, approach_factor,
        protection_factor, zone_factor, pitch_factor, anticipation_factor, aptitude_factor,
        score, probability: sigmoid(score), swung: false,
    }
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
        (((1.0 - sigmoid(bat_control)) + batting_factor.total_modifier) / 2.0).clamp(0.1, 1.2);
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

pub(crate) fn calculate_bat_angle(location: &BallLocation) -> f64 {
    const CENTER_ANGLE_DEG: f64 = 30.0; // Standard tilt angle at zone center
    const HIGH_LOW_RANGE_DEG: f64 = 15.0; // Angle variation range based on height

    // Map norm_y (+1.0 high ~ -1.0 low) to 15° ~ 45°
    let base_angle = CENTER_ANGLE_DEG - (location.y * HIGH_LOW_RANGE_DEG);

    // Clamp to human range of motion limits (10° ~ 60°)
    base_angle.clamp(10.0, 60.0)
}

/// Automatically calculate the effective Attack Angle linked to bat_angle_deg (head drop amount for pitch location)
pub fn calculate_dynamic_attack_angle(attack_angle_deg: f64, bat_angle_deg: f64) -> f64 {
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

// Deterministic angular execution error at the supplied perceived/actual course. No RNG.
pub(crate) fn calculate_execution_angle_error_deg(batter: &BatterInfo, location: &BallLocation) -> f64 {
    let zone_modifier = 1.0 - batter.zone_modifier(location);
    let bat_control_modifier = batter.bat_control / 20.0;

    // 1. Calculate the difference between intended and ideal angle (angle error)
    let intended_location = BallLocation {
        x: location.x + location.x * bat_control_modifier * zone_modifier,
        y: location.y + location.y * bat_control_modifier * zone_modifier,
    };
    let intended_angle = calculate_bat_angle(&intended_location);
    let ideal_angle = calculate_bat_angle(location);

    // Lower contact skill leaves a larger Δθ because the swing can't correct toward the ideal angle
    let unadjusted_ratio = (1.0 - sigmoid(batter.bat_control)).clamp(0.1, 1.0);
    (intended_angle - ideal_angle) * unadjusted_ratio
}

pub fn calculate_swing_execution_error(
    rng: &mut dyn RandomProvider,
    batter: &BatterInfo,
    actual_location: &BallLocation,
) -> SwingExecutionError {
    let ideal_angle = calculate_bat_angle(actual_location);
    let delta_angle_deg = calculate_execution_angle_error_deg(batter, actual_location);

    // 2. Convert angle error (deg) to spatial meter error (Δx, Δz)
    let delta_rad = delta_angle_deg.to_radians();
    const BAT_BARREL_LENGTH_M: f64 = 0.70; // Distance from grip to sweet spot

    let additional_z_m = BAT_BARREL_LENGTH_M * delta_rad.sin();
    let additional_x_m = BAT_BARREL_LENGTH_M * (1.0 - delta_rad.cos());

    // 3. Calculate actual bat angle and attack angle the batter swung
    let actual_bat_angle_deg = ideal_angle + delta_angle_deg;
    // Keep the historical draw so this change does not shift the downstream RNG stream.
    // Batter type shapes preferences/decisions, not an unconditional physical angle bonus.
    let _legacy_attack_angle_sample = rng.normal_factor_std_5_percent();
    let actual_attack_angle_deg =
        calculate_dynamic_attack_angle(batter.attack_angle, actual_bat_angle_deg);

    SwingExecutionError {
        additional_x_m: 0.0,
        additional_z_m: 0.0,
        ideal_bat_angle_deg: ideal_angle,
        actual_bat_angle_deg,
        ideal_attack_angle_deg: batter.attack_angle,
        actual_attack_angle_deg,
    }
}

/// Signed offsets in the bat's X/Z local frame. Magnitudes drive contact checks.
fn project_contact_offsets(x: f64, z: f64, bat_angle_deg: f64) -> (f64, f64) {
    let angle = bat_angle_deg.to_radians();
    (x * angle.cos() + z * angle.sin(), -x * angle.sin() + z * angle.cos())
}

/// Trial horizontal projection of ideal elastic reflection from a cylindrical bat.
/// Incoming reference direction is (0,-1,0); this does not replace EV or VLA.
pub fn contact_normal_hla_deg(signed_thickness_m: f64, bat_angle_deg: f64) -> f64 {
    let radial = (signed_thickness_m / 0.070).clamp(-1.0, 1.0);
    let normal_y = (1.0 - radial * radial).max(0.0).sqrt();
    let normal_x = -bat_angle_deg.to_radians().sin() * radial;
    (2.0 * normal_y * normal_x).atan2(2.0 * normal_y * normal_y - 1.0).to_degrees()
}

fn wrap_hla_deg(angle: f64) -> f64 {
    (angle + 180.0).rem_euclid(360.0) - 180.0
}

// Mismatch between the batter's swing prediction and the actual pitch
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SwingContactResult {
    #[serde(default)]
    pub signed_length_offset_m: Option<f64>,
    #[serde(default)]
    pub signed_thickness_offset_m: Option<f64>,
    #[serde(default)]
    pub timing_position_scale: Option<f64>,
    #[serde(default)]
    pub actual_bat_angle_deg: Option<f64>,
    #[serde(default)]
    pub adapted_horizontal_offset_m: f64,
    #[serde(default)]
    pub adapted_vertical_offset_m: f64,
    #[serde(default)]
    pub swing_error_x_m: f64,
    #[serde(default)]
    pub swing_error_z_m: f64,
    pub timing_offset_sec: f64,
    pub timing_impact_x_m: f64,
    pub offset_x_m: f64,
    pub offset_z_m: f64,
    // NOTE: Spatial sweet-spot offset (0.0: perfectly centered ~ 1.0: completely missing the zone)
    pub thickness_offset_m: f64,
    pub length_offset_m: f64,
    pub original_contact_type: SwingContactType,
    pub adjusted_contact_type: SwingContactType,
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
    MarginalContact,
    // NOTE: Bat swung through air completely (swing and miss)
    SwungAndMiss,
    #[default]
    Take,
}

fn outer_tip_factor(normalized_tip_offset: f64) -> f64 {
    const START: f64 = 0.85;

    let t = ((normalized_tip_offset - START) / (1.0 - START)).clamp(0.0, 1.0);

    // Change smoothly from 0 to 1.
    t * t * (3.0 - 2.0 * t)
}

fn marginal_probability_from_tip_offset(normalized_tip_offset: f64) -> f64 {
    const MAX_PROBABILITY: f64 = 0.70;
    const MIDPOINT: f64 = 0.80;
    const STEEPNESS: f64 = 12.0;

    MAX_PROBABILITY / (1.0 + (-STEEPNESS * (normalized_tip_offset - MIDPOINT)).exp())
}

pub fn evaluate_swing_contact(
    rng: &mut dyn RandomProvider,
    batter: &BatterInfo,
    offset: &PitchDisplacement,
    swing_execution_error: &SwingExecutionError,
) -> SwingContactResult {
    evaluate_swing_contact_with_timing_scale(
        rng, batter, offset, swing_execution_error, CONTACT_TIMING_POSITION_SCALE,
    )
}

fn evaluate_swing_contact_with_timing_scale(
    rng: &mut dyn RandomProvider,
    batter: &BatterInfo,
    offset: &PitchDisplacement,
    swing_execution_error: &SwingExecutionError,
    timing_position_scale: f64,
) -> SwingContactResult {
    assert!(timing_position_scale.is_finite() && timing_position_scale > 0.0);
    // Scale displacement only. Keep timing_offset_sec unchanged for face-angle generation.
    let timing_impact_x_m = batter.swing_speed * offset.timing_offset_sec * timing_position_scale;
    let offset_x_m =
        offset.horizontal_offset_m + swing_execution_error.additional_x_m + timing_impact_x_m;
    let offset_z_m = offset.vertical_offset_m + swing_execution_error.additional_z_m;

    let (signed_length_offset_m, signed_thickness_offset_m) = project_contact_offsets(
        offset_x_m, offset_z_m, swing_execution_error.actual_bat_angle_deg,
    );
    let length_offset_m = signed_length_offset_m.abs();
    let thickness_offset_m = signed_thickness_offset_m.abs();
    let normalized_tip_offset = (length_offset_m / MAX_LENGTH_OFFSET_M).clamp(0.0, 1.0);
    let outer_tip = outer_tip_factor(normalized_tip_offset);

    let marginal_base_prob = marginal_probability_from_tip_offset(normalized_tip_offset);

    // TODO: Bat length should be passed in as a parameter to allow for different bat lengths (e.g. youth vs adult)
    // 1. Bat length limit (e.g. miss if more than 35cm from the sweet spot toward the end)
    let (original_contact_type, adjusted_contact_type) = if length_offset_m > MAX_LENGTH_OFFSET_M {
        (
            SwingContactType::SwungAndMiss,
            SwingContactType::SwungAndMiss,
        )

    // 2. Bat thickness direction (bat radius 3.3cm + ball radius 3.7cm = 7.0cm limit)
    } else if thickness_offset_m > 0.070 {
        (
            SwingContactType::SwungAndMiss,
            SwingContactType::SwungAndMiss,
        )
    } else if thickness_offset_m > 0.055 {
        (
            SwingContactType::MarginalContact,
            SwingContactType::MarginalContact,
        )
    } else if thickness_offset_m > 0.025 {
        let weak_to_marginal_probability = (marginal_base_prob + 0.15 * outer_tip).clamp(0.0, 1.0);
        let adjusted = if rng.random_bool(weak_to_marginal_probability) {
            SwingContactType::MarginalContact
        } else {
            SwingContactType::WeakContact
        };

        (SwingContactType::WeakContact, adjusted)
    } else {
        let solid_to_weak_probability =
            (marginal_base_prob * 0.5 + 0.10 * outer_tip).clamp(0.0, 1.0);
        let adjusted = if rng.random_bool(solid_to_weak_probability) {
            SwingContactType::WeakContact
        } else {
            SwingContactType::SolidContact
        };

        (SwingContactType::SolidContact, adjusted)
    };

    SwingContactResult {
        signed_length_offset_m: Some(signed_length_offset_m),
        signed_thickness_offset_m: Some(signed_thickness_offset_m),
        timing_position_scale: Some(timing_position_scale),
        actual_bat_angle_deg: Some(swing_execution_error.actual_bat_angle_deg),
        adapted_horizontal_offset_m: offset.horizontal_offset_m,
        adapted_vertical_offset_m: offset.vertical_offset_m,
        swing_error_x_m: swing_execution_error.additional_x_m,
        swing_error_z_m: swing_execution_error.additional_z_m,
        timing_offset_sec: offset.timing_offset_sec,
        timing_impact_x_m: timing_impact_x_m,
        offset_x_m: offset_x_m,
        offset_z_m: offset_z_m,
        thickness_offset_m: thickness_offset_m,
        length_offset_m: length_offset_m,
        original_contact_type: original_contact_type,
        adjusted_contact_type: adjusted_contact_type,
        attack_angle_deg: swing_execution_error.actual_attack_angle_deg,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BattedBallAngles {
    pub vla_deg: f64, // Vertical launch angle (deg): + upward pop / - grounder
    pub hla_deg: f64, // Horizontal launch angle (deg): - pull / + opposite (right-handed batter reference)
    pub approach_variation_deg: f64,
    pub pull_bias_deg: f64,
    pub timing_angle_error_deg: f64,
    pub angular_velocity_rad_s: f64,
}

fn calculate_timing_angle_error(
    timing_error_s: f64,
    bat_speed_mps: f64,
    swing_path_radius_m: f64,
) -> f64 {
    let angular_velocity_rad_s = bat_speed_mps / swing_path_radius_m;

    angular_velocity_rad_s * timing_error_s
}

/// Vertical deflection used by the live launch-angle calculation (degrees).
pub fn calculate_vertical_contact_deflection(offset_z_m: f64) -> f64 {
    const COLLISION_RADIUS_M: f64 = 0.070; // Bat radius + ball radius
    let clamped_z = offset_z_m.clamp(-COLLISION_RADIUS_M, COLLISION_RADIUS_M);
    (clamped_z / COLLISION_RADIUS_M).asin().to_degrees() * 0.60
}

pub fn calculate_launch_angles(
    rng: &mut dyn RandomProvider,
    contact: &SwingContactResult,
    batter: &BatterInfo,
) -> Result<BattedBallAngles, GameError> {
    // Approximate distance from the swing's center of rotation to the standard contact point.
    // Used to derive the angular velocity from the bat speed. (1.1m)
    // TODO: Consider adjusting this value based on the batter's height and swing mechanics for more accurate modeling.
    const SWING_PATH_RADIUS_M: f64 = 1.10;

    // 1. Calculate VLA (vertical launch angle)
    // Clamp z_m to the effective radius and compute arcsin
    let vla_deg = contact.attack_angle_deg + calculate_vertical_contact_deflection(contact.offset_z_m);

    // 2. Calculate HLA (horizontal launch angle)
    let timing_error_sec = contact.timing_offset_sec;

    // TODO: bat control should impact the variation range.
    let approach_variation_rad = rng.normal_std(5.0).clamp(-10.0, 10.0).to_radians();
    // TODO: pull_bias_rad should be impacted by batter type and batter strategy.
    let pull_bias_rad = rng.normal_std(5.0).clamp(-10.0, 10.0).to_radians()
        * batter.batting_side.handedness_sign()
        * -1.0;

    let intended_face_angle_rad = approach_variation_rad + pull_bias_rad;
    let timing_angle_error_rad =
        calculate_timing_angle_error(timing_error_sec, batter.swing_speed, SWING_PATH_RADIUS_M);
    let angular_velocity_rad_s = if timing_error_sec.abs() > f64::EPSILON {
        timing_angle_error_rad / timing_error_sec
    } else {
        batter.swing_speed / SWING_PATH_RADIUS_M
    };

    let face_angle_rad = intended_face_angle_rad + timing_angle_error_rad;

    // Use the same local frame as the contact checks, retaining which side was hit.
    let bat_angle = contact.actual_bat_angle_deg.unwrap_or(0.0);
    let (_, signed_thickness) = project_contact_offsets(contact.offset_x_m, contact.offset_z_m, bat_angle);
    let rebound_hla = contact_normal_hla_deg(signed_thickness, bat_angle);
    let raw_hla_deg = face_angle_rad.to_degrees() * 0.85 + rebound_hla;
    let hla_deg = wrap_hla_deg(raw_hla_deg * batter.batting_side.handedness_sign());

    Ok(BattedBallAngles {
        vla_deg,
        hla_deg,
        approach_variation_deg: approach_variation_rad.to_degrees(),
        pull_bias_deg: pull_bias_rad.to_degrees(),
        timing_angle_error_deg: timing_angle_error_rad.to_degrees(),
        angular_velocity_rad_s,
    })
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
        let normalized_dist = (contact_result.thickness_offset_m - SWEET_SPOT_RADIUS_M)
            / (MAX_CONTACT_RADIUS_M - SWEET_SPOT_RADIUS_M);
        (normalized_dist * std::f64::consts::FRAC_PI_2)
            .cos()
            .powi(2)
    };

    // 3. Length-direction energy decay rate (E_len: 0.0 ~ 1.0)
    // Decays as distance from the sweet spot approaches 35cm (handle/tip)
    const MAX_LENGTH_OFFSET_M: f64 = 0.35;
    let e_len =
        (1.0 - (contact_result.length_offset_m / MAX_LENGTH_OFFSET_M).powi(2)).clamp(0.0, 1.0);

    // 4. Calculate final batted ball exit velocity
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
    calculate_collision_spin_from_offsets(
        contact.length_offset_m,
        contact.thickness_offset_m,
        swing_speed,
        ball.spin_rate,
        ball.spin_angle,
    )
}

/// Shared live spin calculation for deterministic replay of saved contact geometry.
pub fn calculate_collision_spin_from_offsets(
    length_offset_m: f64,
    thickness_offset_m: f64,
    swing_speed: f64,
    pitch_spin_rate: f64,
    pitch_spin_angle: f64,
) -> (f64, f64) {
    // 1. Calculate total distance from sweet spot using Pythagorean theorem
    let distance = (length_offset_m.powi(2) + thickness_offset_m.powi(2))
        .sqrt()
        .min(1.0);

    // 2. Calculate contact point angle (radians) using atan2(y, x)
    // y = vertical, x = horizontal
    let impact_angle_rad = thickness_offset_m.atan2(length_offset_m);

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
        raw_spin_rate,
        spin_angle_deg,
        pitch_spin_rate,
        pitch_spin_angle,
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

pub fn calculate_trajectory(
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
    // Preserve the sign: |HLA| > 90 degrees travels behind home plate.
    let mut v_y = launch_speed_ms * vla_rad.cos() * hla_rad.cos();
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

        let fence_distance = stadium.fence_distance_at_angle(current_spray_angle_deg)?;

        // 2. Fence clearing / impact judgment
        if current_distance_m >= fence_distance {
            // A foul-side boundary is out of play, including the backstop.
            // Do not reflect it into fair territory or award a ground-rule double.
            if current_spray_angle_deg.abs() >= FOUL_DEGREE {
                if pos_z > 0.0
                    && pos_z <= stadium.fence_height
                    && fence_impact_position.is_none()
                    && fence_impact_time_sec.is_none()
                {
                    fence_impact_position =
                        Some(PolarPosition::new(fence_distance, current_spray_angle_deg));
                    fence_impact_time_sec = Some(current_time);
                }
                result_type = OutboundResult::Foul;
                break;
            }
            if pos_z > stadium.fence_height {
                result_type = if bounce_count == 0 {
                    OutboundResult::HomeRun
                } else {
                    OutboundResult::GroundRuleDouble
                };
                break;
            } else if pos_z > 0.0 {
                if fence_impact_position.is_none() && fence_impact_time_sec.is_none() {
                    fence_impact_position =
                        Some(PolarPosition::new(fence_distance, current_spray_angle_deg));
                    fence_impact_time_sec = Some(current_time);
                }
                // Fence impact: reverse the velocity in polar terms (bounce back toward the field)
                pos_x = prev_x;
                pos_y = prev_y;
                v_x = -v_x * WALL_RESTITUTION;
                v_y = -v_y * WALL_RESTITUTION;
            }
        }

        // 3. Ground bounce processing
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
    angles: &BattedBallAngles,
    stadium: &Stadium,
) -> Result<BattedBall, GameError> {
    // 1. Calculate exit velocity (damped by spatial offset & timing delay)
    let launch_speed_ms = calculate_launch_speed_with_power(
        contact,
        ball.speed,
        batter.swing_speed,
        batter.swing_power,
    );

    // 2. Calculate batted ball spin
    // Inherit a small portion of the residual spin from pitch.spin_rate / pitch.spin_angle
    let (batted_spin_rate, batted_spin_angle) =
        calculate_collision_spin(ball, batter.swing_speed, contact);

    calculate_trajectory(
        launch_speed_ms,
        angles.vla_deg,
        angles.hla_deg,
        batted_spin_rate,
        batted_spin_angle,
        stadium,
    )
}

pub fn calculate_batted_ball_with_metrics(
    rng: &mut dyn RandomProvider,
    batter: &BatterInfo,
    ball: PitchedBall,
    contact: &SwingContactResult,
    stadium: &Stadium,
) -> Result<(BattedBall, PlayerGameBattingMetrics), GameError> {
    let angles = calculate_launch_angles(rng, contact, batter)?;
    let batted_ball = calculate_batted_ball(batter, ball, contact, &angles, stadium)?;
    let metrics = PlayerGameBattingMetrics {
        hla_model_version: Some(2),
        hla_handedness_sign: Some(batter.batting_side.handedness_sign()),
        bat_speed: batter.swing_speed,
        angular_velocity: angles.angular_velocity_rad_s,
        approach_variation_deg: angles.approach_variation_deg,
        pull_bias_deg: angles.pull_bias_deg,
        timing_angle_error: angles.timing_angle_error_deg,
        final_hla_deg: angles.hla_deg,
        ..PlayerGameBattingMetrics::from_contact(contact)
    };

    Ok((batted_ball, metrics))
}

#[cfg(test)]
mod tests {
    use crate::domain::random_provider::FixedRng;
    use crate::domain::resolver::batting_resolver::*;
    use crate::domain::shared::ball::{BallLocation, PitchedBall, TrajectoryType};
    use crate::domain::shared::game_state::GameError;
    use crate::domain::shared::player::PitcherCharacter;
    use crate::domain::shared::player::{
        ArmSlot, BatterType, FielderInfo, PitchSkill, PitchType, PitcherInfo, PitcherStyle, RL,
        ZoneAptitude,
    };
    use crate::domain::shared::stadium::Stadium;
    use crate::domain::strategy::pitching_strategy::TargetZone;
    use crate::domain::util::Vector3D;

    type TestResult = Result<(), GameError>;

    #[test]
    fn batter_type_does_not_add_a_physical_attack_angle_bonus() {
        let location=BallLocation {x:0.0,y:0.0};
        for kind in [BatterType::AggressiveFreeSwinger,BatterType::ClassicAnalyst,BatterType::GameManager,BatterType::ClutchHunter] {
            let mut batter=batter(RL::Right);batter.batter_type=kind;batter.attack_angle=5.0;
            let mut rng=crate::domain::random_provider::RealRng::from_seed(42);
            let error=calculate_swing_execution_error(&mut rng,&batter,&location);
            assert!((error.actual_attack_angle_deg-calculate_dynamic_attack_angle(5.0,error.actual_bat_angle_deg)).abs()<1e-12);
            let mut reference=crate::domain::random_provider::RealRng::from_seed(42);
            reference.normal_factor_std_5_percent();
            assert_eq!(rng.random(),reference.random(),"legacy RNG consumption changed");
        }
    }

    #[test]
    fn backward_trajectories_preserve_launch_direction_before_bounce() -> TestResult {
        let stadium = Stadium::default();
        // Includes the 23266/158 diagnosis (HLA 157.557 degrees).
        for hla in [
            -180.0_f64, -175.0, -157.557, -135.0, -100.0,
            100.0, 135.0, 157.557, 175.0, 180.0,
        ] {
            let ball = calculate_trajectory(3.3, 12.0, hla, 0.0, 0.0, &stadium)?;
            let bounce = ball.first_bounce_position.expect("weak ball must bounce");
            assert!(bounce.y < 0.0, "HLA {hla}: backward bounce {bounce:?}");
            assert!(bounce.angle.abs() > 90.0);
            assert!(ball.final_position.y < 0.0);
            assert!(ball.is_foul());
            assert_ne!(ball.outbound_result, OutboundResult::GroundRuleDouble);
        }
        Ok(())
    }

    #[test]
    fn backward_backstop_contact_ends_foul_without_forward_reflection() -> TestResult {
        let stadium = Stadium::default();
        for hla in [-175.0, 175.0] {
            let ball = calculate_trajectory(35.0, 0.0, hla, 0.0, 0.0, &stadium)?;
            assert_eq!(ball.outbound_result, OutboundResult::Foul);
            let impact = ball.fence_impact_position.expect("backstop impact");
            assert!(impact.y < 0.0);
            assert!(ball.final_position.y < 0.0);
            assert!(ball.first_bounce_time.is_none());
        }
        Ok(())
    }

    #[test]
    fn fence_clearance_checks_foul_direction_before_bounce_award() -> TestResult {
        // A low fence exercises both airborne and post-bounce clearance.
        let stadium = Stadium::new(1, "Low fence".into(), 20.0, 25.0, 0.0);
        for hla in [-60.0, 0.0, 60.0] {
            for vla in [-10.0, 30.0] {
                let ball = calculate_trajectory(40.0, vla, hla, 0.0, 0.0, &stadium)?;
                assert_eq!(ball.first_bounce_time.is_some(), vla < 0.0);
                let expected = if hla == 0.0 {
                    if vla < 0.0 {
                        OutboundResult::GroundRuleDouble
                    } else {
                        OutboundResult::HomeRun
                    }
                } else {
                    OutboundResult::Foul
                };
                assert_eq!(ball.outbound_result, expected, "HLA {hla}, VLA {vla}");
            }
        }
        Ok(())
    }

    fn batter(batting_side: RL) -> BatterInfo {
        BatterInfo {
            batting_side,
            batter_type: BatterType::ClassicAnalyst,
            zone_aptitude: ZoneAptitude::Balanced,
            hot_zone_scale: 0.1,
            batting_eye: 0.5,
            swing_speed: 41.67,
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
            pitcher_character: PitcherCharacter::Balanced,
            velocity: 41.67,
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
                    velocity: 41.67,
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
                    velocity: 37.5,
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

    #[test]
    fn swing_diagnostics_preserve_components_for_contact_and_miss() {
        for horizontal_offset_m in [0.01, 0.30] {
            let mut rng = FixedRng::new(0.5);
            let batter = batter(RL::Right);
            let offset = PitchDisplacement {
                horizontal_offset_m,
                vertical_offset_m: -0.02,
                timing_offset_sec: -0.001,
                ..Default::default()
            };
            let execution = SwingExecutionError {
                additional_x_m: -0.005,
                additional_z_m: 0.003,
                actual_bat_angle_deg: 0.0,
                ..Default::default()
            };
            let contact = evaluate_swing_contact(&mut rng, &batter, &offset, &execution);
            let metrics = PlayerGameBattingMetrics::from_contact(&contact);
            assert_eq!(
                metrics.adapted_horizontal_offset_m,
                Some(horizontal_offset_m)
            );
            assert_eq!(metrics.adapted_vertical_offset_m, Some(-0.02));
            assert_eq!(metrics.adapted_timing_offset_sec, Some(-0.001));
            assert_eq!(metrics.timing_error, -0.001);
            assert_eq!(metrics.actual_bat_angle_deg, Some(0.0));
            assert_eq!(metrics.swing_error_x_m, Some(-0.005));
            assert_eq!(metrics.swing_error_z_m, Some(0.003));
            assert_eq!(metrics.timing_impact_x_m, Some(-0.001 * batter.swing_speed * CONTACT_TIMING_POSITION_SCALE));
            assert_eq!(metrics.contact_offset_x_m, Some(contact.offset_x_m));
            assert_eq!(metrics.contact_offset_z_m, Some(contact.offset_z_m));
            assert_eq!(metrics.thickness_offset_m, Some(contact.thickness_offset_m));
            assert!(
                (contact.offset_x_m - (horizontal_offset_m - 0.005 - 0.001 * batter.swing_speed * CONTACT_TIMING_POSITION_SCALE))
                    .abs()
                    < 1e-12
            );
            assert!((contact.offset_z_m + 0.017).abs() < 1e-12);
            assert_eq!(
                matches!(
                    metrics.adjusted_contact_type,
                    SwingContactType::SwungAndMiss
                ),
                horizontal_offset_m > 0.16
            );
        }
        assert_eq!(PlayerGameBattingMetrics::default().contact_offset_x_m, None);
    }

    #[test]
    fn legacy_contact_without_bat_angle_stays_unknown() {
        let contact = centered_contact(5.0);
        let mut json = serde_json::to_value(&contact).unwrap();
        json.as_object_mut().unwrap().remove("actual_bat_angle_deg");
        let legacy: SwingContactResult = serde_json::from_value(json).unwrap();
        assert_eq!(legacy.actual_bat_angle_deg, None);
        assert_eq!(PlayerGameBattingMetrics::from_contact(&legacy).actual_bat_angle_deg, None);
    }

    fn centered_contact(attack_angle_deg: f64) -> SwingContactResult {
        SwingContactResult {
            timing_offset_sec: 0.0,
            timing_impact_x_m: 0.0,
            offset_x_m: 0.0,
            offset_z_m: 0.0,
            thickness_offset_m: 0.0,
            length_offset_m: 0.0,
            original_contact_type: SwingContactType::SolidContact,
            adjusted_contact_type: SwingContactType::SolidContact,
            attack_angle_deg,
            ..Default::default()
        }
    }

    #[test]
    fn calculate_launch_angles_uses_contact_offsets_and_batting_side() -> TestResult {
        let mut contact = centered_contact(5.0);
        contact.actual_bat_angle_deg = Some(30.0);
        contact.offset_x_m = -0.0175;
        contact.offset_z_m = 0.035 * 30.0_f64.to_radians().cos();
        let right = calculate_launch_angles(&mut FixedRng::new(0.5), &contact, &batter(RL::Right))?;
        let left = calculate_launch_angles(&mut FixedRng::new(0.5), &contact, &batter(RL::Left))?;
        assert!((right.hla_deg + 40.8933946491309).abs() < 1e-8);
        assert!((right.hla_deg + left.hla_deg).abs() < 1e-8);
        assert_eq!(right.vla_deg, left.vla_deg);
        assert_eq!(right.timing_angle_error_deg, 0.0);
        Ok(())
    }

    #[test]
    fn calculate_launch_angles_derives_angular_velocity_from_timing_error() -> TestResult {
        let contact = SwingContactResult {
            timing_offset_sec: 0.01,
            ..centered_contact(5.0)
        };

        let mut rng = FixedRng::new(0.5);
        let angles = calculate_launch_angles(&mut rng, &contact, &batter(RL::Right))?;
        let timing_angle_error_rad = angles.timing_angle_error_deg.to_radians();

        assert!(
            (angles.angular_velocity_rad_s - timing_angle_error_rad / contact.timing_offset_sec)
                .abs()
                < f64::EPSILON
        );
        assert!(angles.approach_variation_deg.abs() < f64::EPSILON);
        assert!(angles.pull_bias_deg.abs() < f64::EPSILON);

        Ok(())
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
    fn calculate_batting_factor_penalizes_locations_outside_target_zones() {
        assert_eq!(
            batting_factor_for_locations(
                BallLocation {
                    x: -0.7469886858243414,
                    y: 1.3173337509815242,
                },
                BallLocation { x: 0.0, y: 0.0 },
            )
            .zone_similarity,
            -0.2
        );
    }

    #[test]
    fn calculate_batted_ball_sets_physical_values() -> TestResult {
        let right_pull_hitter = batter(RL::Right);
        let stadium = Stadium::default();

        for _ in 0..50 {
            let contact = centered_contact(right_pull_hitter.attack_angle);
            let mut rng = FixedRng::new(0.5);
            let angles = calculate_launch_angles(&mut rng, &contact, &right_pull_hitter)?;
            let ball = calculate_batted_ball(
                &right_pull_hitter,
                PitchedBall {
                    pitch_type: PitchType::FourSeamFastball,
                    speed: 41.67,
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
                &contact,
                &angles,
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

#[cfg(test)]
mod swing_decision_tests {
    use super::*;
    fn factor(outside: f64) -> BattingFactor {
        BattingFactor { distance_from_zone_edge: outside, zone_similarity: -0.2,
            pitch_similarity: -0.2, zone_aptitude: 0.1, total_modifier: 0.0 }
    }
    #[test]
    fn protecting_increases_zone_swing_probability_for_each_ball_count() {
        for (one, two) in [(CountStatus::C01, CountStatus::C02),
            (CountStatus::C11, CountStatus::C12), (CountStatus::C21, CountStatus::C22),
            (CountStatus::C31, CountStatus::C32)] {
            let before = calculate_swing_decision(PlateApproach::Aggressive, one, PitchType::FourSeamFastball, &factor(0.0));
            let after = calculate_swing_decision(PlateApproach::Aggressive, two, PitchType::FourSeamFastball, &factor(0.0));
            assert!(after.probability > before.probability);
            assert_eq!(after.approach_factor, 0.2);
            assert!((after.anticipation_factor + 0.1).abs() < 1e-12);
            let sum = after.count_factor + after.approach_factor + after.protection_factor
                + after.zone_factor + after.pitch_factor + after.anticipation_factor + after.aptitude_factor;
            assert!((sum - after.score).abs() < 1e-12);
        }
    }
    #[test]
    fn farther_balls_are_less_attractive_even_with_two_strikes() {
        for count in [CountStatus::C00, CountStatus::C02, CountStatus::C12, CountStatus::C22, CountStatus::C32] {
            let probabilities: Vec<_> = [0.0, 0.1, 0.5, 1.0, 2.0].iter().map(|d|
                calculate_swing_decision(PlateApproach::Aggressive, count, PitchType::FourSeamFastball, &factor(*d)).probability
            ).collect();
            assert!(probabilities.windows(2).all(|p| p[0] > p[1]));
        }
    }
    #[test]
    fn pre_two_strike_take_intent_is_preserved_but_protection_can_override_it() {
        let early = calculate_swing_decision(PlateApproach::Take, CountStatus::C01, PitchType::FourSeamFastball, &factor(0.0));
        let late = calculate_swing_decision(PlateApproach::Take, CountStatus::C02, PitchType::FourSeamFastball, &factor(0.0));
        assert_eq!(early.approach_factor, -5.0);
        assert_eq!(late.approach_factor, 0.0);
        assert!(late.probability > early.probability);
    }
}

#[cfg(test)]
mod stronger_zone_protection_tests {
    use super::*;
    fn decision(count: CountStatus, distance: f64) -> SwingDecisionDiagnostics {
        calculate_swing_decision(PlateApproach::Aggressive, count, PitchType::FourSeamFastball,
            &BattingFactor { distance_from_zone_edge: distance, zone_similarity: -0.2,
                pitch_similarity: -0.2, zone_aptitude: 0.1, total_modifier: 0.0 })
    }
    #[test]
    fn zone_protection_is_strong_without_changing_other_score_components() {
        for count in [CountStatus::C02, CountStatus::C12, CountStatus::C22, CountStatus::C32] {
            let d = decision(count, 0.0);
            assert_eq!(d.protection_factor, 1.75);
            assert!(d.probability > 0.88 && d.probability < 0.93);
            let other = d.count_factor + d.approach_factor + d.zone_factor + d.pitch_factor
                + d.anticipation_factor + d.aptitude_factor;
            assert!((d.score - other - 1.75).abs() < 1e-12);
        }
    }
    #[test]
    fn distant_balls_receive_no_more_protection_than_before() {
        for distance in [0.25, 0.5, 1.0, 2.0] {
            let d = decision(CountStatus::C02, distance);
            assert!(d.protection_factor < 0.35 / (1.0 + 4.0 * distance));
        }
        let probabilities: Vec<_> = [0.0, 1e-8, 0.05, 0.1, 0.25, 0.5, 1.0].iter()
            .map(|d| decision(CountStatus::C02, *d).probability).collect();
        assert!(probabilities.windows(2).all(|p| p[0] > p[1]));
        assert!((probabilities[0] - probabilities[1]).abs() < 1e-6);
    }
    #[test]
    fn early_counts_have_no_protection_bonus() {
        for count in [CountStatus::C00, CountStatus::C01, CountStatus::C10, CountStatus::C11,
            CountStatus::C20, CountStatus::C21, CountStatus::C30, CountStatus::C31] {
            let d = decision(count, 0.1);
            assert_eq!(d.protection_factor, 0.0);
            let expected = count.prob() + 0.2 - 0.2 + 0.1 - 0.4 + 0.1;
            assert!((d.score - expected).abs() < 1e-12);
        }
    }
}

#[cfg(test)]
mod timing_position_trial_tests {
    use super::*;
    use crate::domain::random_provider::FixedRng;
    #[test]
    fn displacement_scales_but_time_and_face_rotation_do_not() {
        let mut batter = crate::domain::test_support::batter_info(crate::domain::shared::player::RL::Right);
        batter.swing_speed = 40.0;
        let offset = PitchDisplacement { timing_offset_sec: 0.005, ..Default::default() };
        let execution = SwingExecutionError { actual_bat_angle_deg: 30.0, ..Default::default() };
        let control = evaluate_swing_contact_with_timing_scale(&mut FixedRng::new(0.5), &batter, &offset, &execution, 1.0);
        let trial = evaluate_swing_contact_with_timing_scale(&mut FixedRng::new(0.5), &batter, &offset, &execution, 0.75);
        assert_eq!(trial.timing_offset_sec, control.timing_offset_sec);
        assert!((trial.timing_impact_x_m - 0.15).abs() < 1e-12);
        assert!((trial.offset_x_m - control.offset_x_m * 0.75).abs() < 1e-12);
        assert_eq!(trial.actual_bat_angle_deg, control.actual_bat_angle_deg);
        assert_eq!(trial.timing_position_scale, Some(0.75));
        assert_eq!(calculate_timing_angle_error(trial.timing_offset_sec, 40.0, 1.1),
                   calculate_timing_angle_error(control.timing_offset_sec, 40.0, 1.1));
    }
}

#[cfg(test)]
mod contact_normal_tests {
    use super::*;
    use crate::domain::random_provider::FixedRng;
    use crate::domain::shared::player::RL;
    #[test]
    fn local_projection_preserves_magnitude_and_signed_side() {
        for angle in [0.0, 15.0, 30.0, 60.0] {
            let (l,t) = project_contact_offsets(0.12,-0.03,angle);
            assert!((l*l+t*t-(0.12_f64.powi(2)+0.03_f64.powi(2))).abs()<1e-12);
            let opposite=project_contact_offsets(-0.12,0.03,angle);
            assert!((l+opposite.0).abs()<1e-12 && (t+opposite.1).abs()<1e-12);
        }
    }
    #[test]
    fn centred_contacts_are_forward_and_grazing_contacts_can_go_foul() {
        assert_eq!(contact_normal_hla_deg(0.0,30.0),0.0);
        assert!(contact_normal_hla_deg(0.05,30.0).abs()>45.0);
        for offset in [-0.070,-0.05,-0.02,0.0,0.02,0.05,0.070] {
            assert!(contact_normal_hla_deg(offset,30.0).is_finite());
        }
    }
    #[test]
    fn contact_label_does_not_replace_original_timing() {
        let batter=crate::domain::test_support::batter_info(RL::Right);
        let mut contact=SwingContactResult { actual_bat_angle_deg:Some(30.0),
            offset_x_m:0.01,offset_z_m:0.02,timing_offset_sec:0.003,
            adjusted_contact_type:SwingContactType::WeakContact,..Default::default() };
        let weak=calculate_launch_angles(&mut FixedRng::new(0.5),&contact,&batter).unwrap();
        contact.adjusted_contact_type=SwingContactType::MarginalContact;
        let marginal=calculate_launch_angles(&mut FixedRng::new(0.5),&contact,&batter).unwrap();
        assert_eq!(weak.hla_deg,marginal.hla_deg);
        assert_eq!(weak.timing_angle_error_deg,marginal.timing_angle_error_deg);
    }
}
