use super::batting_calibration::{BAT_CONTROL_CORRECTION_MIDPOINT, uncorrected_fraction};
use crate::domain::shared::flight_drag::{self, FlightDragModel};
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
/// Absolute bat-local displacement limits used by the geometric contact check.
pub const CONTACT_LENGTH_LIMIT_M: f64 = 0.16;
pub const CONTACT_THICKNESS_LIMIT_M: f64 = 0.070;
/// Trial setting: change to 1.0 for the unscaled control; recorded on every swing.
pub const CONTACT_TIMING_POSITION_SCALE: f64 = 0.75;

thread_local! {
    static CONTACT_GEOMETRY_SCALE: std::cell::Cell<f64> =
        const { std::cell::Cell::new(CONTACT_TIMING_POSITION_SCALE) };
}

/// Set the timing-to-position scale for a synchronous full-game diagnostic.
/// The exit-speed depth correction still uses the default scale.
pub fn with_contact_geometry_scale<T>(scale: f64, run: impl FnOnce() -> T) -> T {
    assert!(scale.is_finite() && scale > 0.0);
    struct Restore(f64);
    impl Drop for Restore {
        fn drop(&mut self) {
            CONTACT_GEOMETRY_SCALE.set(self.0);
        }
    }
    let _restore = Restore(CONTACT_GEOMETRY_SCALE.replace(scale));
    run()
}
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

impl SwingDecisionDiagnostics {
    pub(crate) fn apply_edge_take_trial(&mut self, location: BallLocation) {
        let depth = 1.0 - location.x.abs().max(location.y.abs());
        // Ramp in from the zone core and fade out on the ball side of the edge.
        let edge_weight = if depth >= 0.0 {
            (1.0 - depth / 0.20).max(0.0)
        } else {
            (1.0 - location.distance_from_zone_edge() / 0.50).max(0.0)
        };
        let count_penalty = match self.count_status {
            CountStatus::C30 | CountStatus::C31 => 0.80,
            CountStatus::C20 | CountStatus::C21 => 0.65,
            CountStatus::C00 | CountStatus::C10 | CountStatus::C11 => 0.45,
            CountStatus::C01 => 0.35,
            CountStatus::C02 | CountStatus::C12 | CountStatus::C22 | CountStatus::C32 => 0.20,
        };
        let penalty = count_penalty * edge_weight;
        self.zone_factor -= penalty;
        self.score -= penalty;
        self.probability = sigmoid(self.score);
    }
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
    let residual = uncorrected_fraction(bat_control, BAT_CONTROL_CORRECTION_MIDPOINT);
    let total_modifier = ((residual + batting_factor.total_modifier) / 2.0).clamp(0.1, 1.2);
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

pub const BAT_ANGLE_ATTACK_COUPLING: f64 = 0.35;

/// Automatically calculate the effective Attack Angle linked to bat_angle_deg (head drop amount for pitch location)
pub fn calculate_dynamic_attack_angle(attack_angle_deg: f64, bat_angle_deg: f64) -> f64 {
    // Baseline bat_angle_deg (e.g. 30° as the standard tilt)
    const BASE_BAT_ANGLE_DEG: f64 = 30.0;

    let angle_delta = bat_angle_deg - BASE_BAT_ANGLE_DEG;

    // Determine the dynamic attack angle
    attack_angle_deg + (angle_delta * BAT_ANGLE_ATTACK_COUPLING)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwingExecutionModel {
    #[default]
    Legacy,
    Gaussian,
    Mixture,
}

thread_local! {
    static SWING_EXECUTION_MODEL: std::cell::Cell<SwingExecutionModel> =
        const { std::cell::Cell::new(SwingExecutionModel::Legacy) };
    static FAST_SWING_MAX_ERROR_SCALE: std::cell::Cell<f64> =
        const { std::cell::Cell::new(1.0) };
    static COLLISION_POWER_TRIAL: std::cell::Cell<Option<(f64, f64)>> =
        const { std::cell::Cell::new(None) };
}

/// Retain a fraction of each batter's collision coefficient difference from the roster median.
pub fn with_collision_power_trial<T>(
    median_power: f64,
    retained_fraction: f64,
    run: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<(f64, f64)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            COLLISION_POWER_TRIAL.set(self.0);
        }
    }
    let trial = (retained_fraction != 1.0).then(|| (sigmoid(median_power), retained_fraction));
    let _restore = Restore(COLLISION_POWER_TRIAL.replace(trial));
    run()
}

pub const FAST_SWING_ERROR_START_MS: f64 = 34.0;
pub const FAST_SWING_ERROR_FULL_MS: f64 = 35.5;

/// Temporarily widen the stochastic execution errors for fast swings.
pub fn with_fast_swing_error_trial<T>(max_scale: f64, run: impl FnOnce() -> T) -> T {
    struct Restore(f64);
    impl Drop for Restore {
        fn drop(&mut self) {
            FAST_SWING_MAX_ERROR_SCALE.set(self.0);
        }
    }
    let _restore = Restore(FAST_SWING_MAX_ERROR_SCALE.replace(max_scale));
    run()
}

/// Select the execution model for a synchronous simulation or fixed-pitch trial.
pub fn with_swing_execution_model<T>(model: SwingExecutionModel, run: impl FnOnce() -> T) -> T {
    struct Restore(SwingExecutionModel);
    impl Drop for Restore {
        fn drop(&mut self) {
            SWING_EXECUTION_MODEL.set(self.0);
        }
    }
    let _restore = Restore(SWING_EXECUTION_MODEL.replace(model));
    run()
}

fn swing_execution_scale(bat_control: f64) -> f64 {
    let residual = 1.0 - sigmoid(bat_control - BAT_CONTROL_CORRECTION_MIDPOINT);
    // Keep an execution floor even when the batter can correct most prediction errors.
    0.55_f64.hypot(1.25 * residual)
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct SwingExecutionError {
    pub additional_timing_sec: f64,
    pub additional_x_m: f64,
    pub additional_z_m: f64,
    pub ideal_bat_angle_deg: f64,
    pub actual_bat_angle_deg: f64,
    pub ideal_attack_angle_deg: f64,
    pub actual_attack_angle_deg: f64,
}

impl SwingExecutionError {
    pub(crate) fn with_rescaled_offsets(
        mut self,
        source: Self,
        original: &BatterInfo,
        trial: &BatterInfo,
    ) -> Self {
        let ratio =
            swing_execution_scale(trial.bat_control) / swing_execution_scale(original.bat_control);
        self.additional_timing_sec = source.additional_timing_sec * ratio;
        self.additional_x_m = source.additional_x_m * ratio;
        self.additional_z_m = source.additional_z_m * ratio;
        self
    }
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
    // The legacy model keeps its historical RNG consumption.
    let _legacy_attack_angle_sample = rng.normal_factor_std_5_percent();
    let mut error = swing_execution_geometry(batter, actual_location);
    let model = SWING_EXECUTION_MODEL.get();
    if model == SwingExecutionModel::Legacy {
        return error;
    }

    // Share the width across axes for a given swing. Normalize the mixture to the
    // same variance as the Gaussian so the comparison isolates the shape.
    const WIDE_SWING_RATE: f64 = 0.10;
    const WIDE_SWING_MULTIPLIER: f64 = 2.25;
    // Both stochastic models draw the same samples for a fixed seed.
    let wide_swing = rng.random_bool(WIDE_SWING_RATE);
    let width = if model == SwingExecutionModel::Mixture {
        let variance = 1.0 + WIDE_SWING_RATE * (WIDE_SWING_MULTIPLIER.powi(2) - 1.0);
        let multiplier = if wide_swing {
            WIDE_SWING_MULTIPLIER
        } else {
            1.0
        };
        multiplier / variance.sqrt()
    } else {
        1.0
    };
    let mut scale = swing_execution_scale(batter.bat_control) * width;
    let max_fast_scale = FAST_SWING_MAX_ERROR_SCALE.get();
    if max_fast_scale != 1.0 {
        let fraction = ((batter.swing_speed - FAST_SWING_ERROR_START_MS)
            / (FAST_SWING_ERROR_FULL_MS - FAST_SWING_ERROR_START_MS))
            .clamp(0.0, 1.0);
        scale *= 1.0 + fraction * (max_fast_scale - 1.0);
    }
    error.additional_timing_sec = scale * rng.normal_std(0.002);
    error.additional_x_m = scale * rng.normal_std(0.015);
    error.additional_z_m = scale * rng.normal_std(0.020);
    error
}

pub(crate) fn swing_execution_geometry(
    batter: &BatterInfo,
    actual_location: &BallLocation,
) -> SwingExecutionError {
    let ideal_angle = calculate_bat_angle(actual_location);
    let delta_angle_deg = calculate_execution_angle_error_deg(batter, actual_location);
    // Batter type shapes preferences and decisions without a physical angle bonus.
    let actual_bat_angle_deg = ideal_angle + delta_angle_deg;
    let actual_attack_angle_deg =
        calculate_dynamic_attack_angle(batter.attack_angle, actual_bat_angle_deg);

    SwingExecutionError {
        additional_timing_sec: 0.0,
        additional_x_m: 0.0,
        additional_z_m: 0.0,
        ideal_bat_angle_deg: ideal_angle,
        actual_bat_angle_deg,
        ideal_attack_angle_deg: batter.attack_angle,
        actual_attack_angle_deg,
    }
}

/// Signed offsets in the bat's X/Z local frame. Magnitudes drive contact checks.
pub fn project_contact_offsets(x: f64, z: f64, bat_angle_deg: f64) -> (f64, f64) {
    let angle = bat_angle_deg.to_radians();
    (x * angle.cos() + z * angle.sin(), -x * angle.sin() + z * angle.cos())
}

/// World Z intervals allowed by the length and thickness limits, respectively, at fixed X.
pub fn vertical_contact_ranges_at_x(x: f64, bat_angle_deg: f64) -> Option<[(f64, f64); 2]> {
    let axis_range = |intercept: f64, slope: f64, limit: f64| {
        if slope == 0.0 {
            return (intercept.abs() <= limit).then_some((f64::NEG_INFINITY, f64::INFINITY));
        }
        let first = (-limit - intercept) / slope;
        let second = (limit - intercept) / slope;
        Some((first.min(second), first.max(second)))
    };
    let (length_at_zero, thickness_at_zero) = project_contact_offsets(x, 0.0, bat_angle_deg);
    let (length_slope, thickness_slope) = project_contact_offsets(0.0, 1.0, bat_angle_deg);
    Some([
        axis_range(length_at_zero, length_slope, CONTACT_LENGTH_LIMIT_M)?,
        axis_range(thickness_at_zero, thickness_slope, CONTACT_THICKNESS_LIMIT_M)?,
    ])
}

/// Whether the projected bat-frame offsets are outside the live contact limits.
pub fn contact_geometry_is_miss(length_m: f64, thickness_m: f64) -> bool {
    length_m.abs() > CONTACT_LENGTH_LIMIT_M || thickness_m.abs() > CONTACT_THICKNESS_LIMIT_M
}

/// Trial horizontal projection of ideal elastic reflection from a cylindrical bat.
/// Incoming reference direction is (0,-1,0); this does not replace EV or VLA.
pub fn contact_normal_hla_deg(signed_thickness_m: f64, bat_angle_deg: f64) -> f64 {
    let radial = (signed_thickness_m / 0.070).clamp(-1.0, 1.0);
    let normal_y = (1.0 - radial * radial).max(0.0).sqrt();
    let normal_x = -bat_angle_deg.to_radians().sin() * radial;
    (2.0 * normal_y * normal_x).atan2(2.0 * normal_y * normal_y - 1.0).to_degrees()
}

pub fn wrap_hla_deg(angle: f64) -> f64 {
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
    #[serde(default)]
    pub execution_timing_offset_sec: f64,
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
        rng,
        batter,
        offset,
        swing_execution_error,
        CONTACT_GEOMETRY_SCALE.get(),
    )
}

struct SwingContactGeometry {
    timing_offset_sec: f64,
    timing_impact_x_m: f64,
    offset_x_m: f64,
    offset_z_m: f64,
    signed_length_offset_m: f64,
    signed_thickness_offset_m: f64,
}

fn swing_contact_geometry(
    batter: &BatterInfo,
    offset: &PitchDisplacement,
    error: &SwingExecutionError,
    timing_position_scale: f64,
) -> SwingContactGeometry {
    assert!(timing_position_scale.is_finite() && timing_position_scale > 0.0);
    // Scale the geometric displacement, while the face-angle calculation uses the actual timing.
    let timing_offset_sec = offset.timing_offset_sec + error.additional_timing_sec;
    let timing_impact_x_m = batter.swing_speed * timing_offset_sec * timing_position_scale;
    let offset_x_m = offset.horizontal_offset_m + error.additional_x_m + timing_impact_x_m;
    let offset_z_m = offset.vertical_offset_m + error.additional_z_m;
    let (signed_length_offset_m, signed_thickness_offset_m) =
        project_contact_offsets(offset_x_m, offset_z_m, error.actual_bat_angle_deg);
    SwingContactGeometry {
        timing_offset_sec,
        timing_impact_x_m,
        offset_x_m,
        offset_z_m,
        signed_length_offset_m,
        signed_thickness_offset_m,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ContactProtectionTrial {
    EdgeCorrection,
    ReducedCenterCorrection,
    Combined,
}

/// Adjust the realized thickness error in the bat frame, after live aim is applied.
pub(crate) fn apply_contact_protection_trial(
    batter: &BatterInfo,
    displacement: &PitchDisplacement,
    error: &mut SwingExecutionError,
    trial: ContactProtectionTrial,
) {
    let geometry = swing_contact_geometry(batter, displacement, error, CONTACT_GEOMETRY_SCALE.get());
    let signed = geometry.signed_thickness_offset_m;
    let thickness = signed.abs();
    let center = if matches!(
        trial,
        ContactProtectionTrial::ReducedCenterCorrection | ContactProtectionTrial::Combined
    ) {
        0.014 * (thickness / 0.025).min((0.055 - thickness) / 0.030).max(0.0)
    } else {
        0.0
    };
    let edge = if matches!(
        trial,
        ContactProtectionTrial::EdgeCorrection | ContactProtectionTrial::Combined
    ) {
        0.012 * ((thickness - 0.055) / 0.015).min((0.090 - thickness) / 0.020).max(0.0)
    } else {
        0.0
    };
    let shift = (center - edge) * signed.signum();
    let (sin, cos) = error.actual_bat_angle_deg.to_radians().sin_cos();
    error.additional_x_m -= shift * sin;
    error.additional_z_m += shift * cos;
}

/// Exit speed for the realized swing before a diagnostic offset, without drawing RNG.
pub(crate) fn pre_trial_exit_speed_ms(
    batter: &BatterInfo,
    pitch_speed_ms: f64,
    offset: &PitchDisplacement,
    error: &SwingExecutionError,
) -> Option<f64> {
    let geometry = swing_contact_geometry(batter, offset, error, CONTACT_GEOMETRY_SCALE.get());
    let length = geometry.signed_length_offset_m.abs();
    let thickness = geometry.signed_thickness_offset_m.abs();
    if contact_geometry_is_miss(length, thickness) {
        return None;
    }
    // The game keeps the reference depth scale for launch speed during geometry trials.
    let depth = batter.swing_speed * geometry.timing_offset_sec * CONTACT_TIMING_POSITION_SCALE;
    let speed = launch_speed_terms(
        pitch_speed_ms,
        batter.swing_speed,
        batter.swing_power,
        depth,
        thickness,
        length,
    )
    .exit_speed_ms();
    speed.is_finite().then_some(speed)
}

pub(crate) fn evaluate_swing_contact_with_timing_scale(
    rng: &mut dyn RandomProvider,
    batter: &BatterInfo,
    offset: &PitchDisplacement,
    swing_execution_error: &SwingExecutionError,
    timing_position_scale: f64,
) -> SwingContactResult {
    let SwingContactGeometry {
        timing_offset_sec,
        timing_impact_x_m,
        offset_x_m,
        offset_z_m,
        signed_length_offset_m,
        signed_thickness_offset_m,
    } = swing_contact_geometry(batter, offset, swing_execution_error, timing_position_scale);
    let length_offset_m = signed_length_offset_m.abs();
    let thickness_offset_m = signed_thickness_offset_m.abs();
    let normalized_tip_offset = (length_offset_m / CONTACT_LENGTH_LIMIT_M).clamp(0.0, 1.0);
    let outer_tip = outer_tip_factor(normalized_tip_offset);

    let marginal_base_prob = marginal_probability_from_tip_offset(normalized_tip_offset);

    // TODO: Bat length should be passed in as a parameter to allow for different bat lengths (e.g. youth vs adult)
    let (original_contact_type, adjusted_contact_type) = if contact_geometry_is_miss(
        length_offset_m,
        thickness_offset_m,
    ) {
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
        execution_timing_offset_sec: swing_execution_error.additional_timing_sec,
        timing_offset_sec,
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

pub(crate) const VERTICAL_COLLISION_RADIUS_M: f64 = 0.070; // Bat radius + ball radius
const VERTICAL_DEFLECTION_SCALE: f64 = 0.60;

/// Vertical deflection used by the live launch-angle calculation (degrees).
pub fn calculate_vertical_contact_deflection(offset_z_m: f64) -> f64 {
    let clamped_z = offset_z_m.clamp(-VERTICAL_COLLISION_RADIUS_M, VERTICAL_COLLISION_RADIUS_M);
    (clamped_z / VERTICAL_COLLISION_RADIUS_M).asin().to_degrees() * VERTICAL_DEFLECTION_SCALE
}

/// Z must be strictly greater than this boundary to exceed the requested deflection.
/// None means even maximum deflection cannot exceed it.
pub fn vertical_offset_boundary_to_exceed_deflection(deflection_deg: f64) -> Option<f64> {
    let max_deflection_deg = 90.0 * VERTICAL_DEFLECTION_SCALE;
    if deflection_deg >= max_deflection_deg {
        None
    } else if deflection_deg < -max_deflection_deg {
        Some(f64::NEG_INFINITY)
    } else {
        Some(
            VERTICAL_COLLISION_RADIUS_M
                * (deflection_deg / VERTICAL_DEFLECTION_SCALE).to_radians().sin(),
        )
    }
}

/// Draw the same face-angle terms used by a live contact, in radians.
pub fn sample_face_angle_terms(
    rng: &mut dyn RandomProvider,
    handedness_sign: f64,
    swing_speed: f64,
    timing_error_sec: f64,
) -> (f64, f64, f64) {
    const SWING_PATH_RADIUS_M: f64 = 1.10;
    let approach = rng.normal_std(5.0).clamp(-10.0, 10.0).to_radians();
    let pull = rng.normal_std(5.0).clamp(-10.0, 10.0).to_radians() * handedness_sign * -1.0;
    let timing = calculate_timing_angle_error(timing_error_sec, swing_speed, SWING_PATH_RADIUS_M);
    (approach, pull, timing)
}

pub fn calculate_launch_angles(
    rng: &mut dyn RandomProvider,
    contact: &SwingContactResult,
    batter: &BatterInfo,
) -> Result<BattedBallAngles, GameError> {
    const SWING_PATH_RADIUS_M: f64 = 1.10;

    let vla_deg = contact.attack_angle_deg + calculate_vertical_contact_deflection(contact.offset_z_m);
    let timing_error_sec = contact.timing_offset_sec;
    let (approach_variation_rad, pull_bias_rad, timing_angle_error_rad) = sample_face_angle_terms(
        rng, batter.batting_side.handedness_sign(), batter.swing_speed, timing_error_sec,
    );
    let intended_face_angle_rad = approach_variation_rad + pull_bias_rad;
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
    calculate_launch_speed_with_depth(
        contact_result,
        ball_speed,
        swing_speed,
        swing_power,
        contact_result.timing_impact_x_m,
    )
}

pub(crate) fn calculate_launch_speed_with_depth(
    contact_result: &SwingContactResult,
    ball_speed: f64,
    swing_speed: f64,
    swing_power: f64,
    depth_displacement_m: f64,
) -> f64 {
    launch_speed_terms(
        ball_speed,
        swing_speed,
        swing_power,
        depth_displacement_m,
        contact_result.thickness_offset_m,
        contact_result.length_offset_m,
    )
    .exit_speed_ms()
}

/// The pitch and bat contributions before the two contact-offset factors.
pub struct LaunchSpeedTerms {
    pub pitch_term_ms: f64,
    pub swing_term_ms: f64,
    pub thickness_retention: f64,
    pub length_retention: f64,
}

impl LaunchSpeedTerms {
    pub fn exit_speed_ms(&self) -> f64 {
        (self.pitch_term_ms + self.swing_term_ms) * self.thickness_retention * self.length_retention
    }
}

pub fn launch_speed_terms(
    ball_speed: f64,
    swing_speed: f64,
    swing_power: f64,
    depth_displacement_m: f64,
    thickness_offset_m: f64,
    length_offset_m: f64,
) -> LaunchSpeedTerms {
    // 1. Theoretical maximum exit velocity on perfect sweet-spot contact (m/s)
    const C_PITCH: f64 = 0.18; // Pitch speed contribution (18%)
    // Dynamic C_SWING variation based on swing_power (1.12 ~ 1.28)
    let power = sigmoid(swing_power);
    let power = COLLISION_POWER_TRIAL
        .get()
        .map_or(power, |(median, retained)| median + retained * (power - median));
    let power = calculate_effective_c_swing(power, depth_displacement_m);
    let c_swing: f64 = 1.12 + (0.16 * power);

    // 2. Thickness-direction energy decay rate (E_thick: 0.0 ~ 1.0)
    const SWEET_SPOT_RADIUS_M: f64 = 0.020; // Sweet spot radius (2.0cm)
    const MAX_CONTACT_RADIUS_M: f64 = 0.070; // Contact limit (7.0cm)

    let e_thick = if thickness_offset_m <= SWEET_SPOT_RADIUS_M {
        1.0
    } else if thickness_offset_m >= MAX_CONTACT_RADIUS_M {
        0.0
    } else {
        // Smoothly decay between 2cm ~ 7cm using Smoothstep / Cosine
        let normalized_dist = (thickness_offset_m - SWEET_SPOT_RADIUS_M)
            / (MAX_CONTACT_RADIUS_M - SWEET_SPOT_RADIUS_M);
        (normalized_dist * std::f64::consts::FRAC_PI_2)
            .cos()
            .powi(2)
    };

    // 3. Length-direction energy decay rate (E_len: 0.0 ~ 1.0)
    // Decays as distance from the sweet spot approaches 35cm (handle/tip)
    const MAX_LENGTH_OFFSET_M: f64 = 0.35;
    let e_len =
        (1.0 - (length_offset_m / MAX_LENGTH_OFFSET_M).powi(2)).clamp(0.0, 1.0);

    LaunchSpeedTerms {
        pitch_term_ms: C_PITCH * ball_speed,
        swing_term_ms: c_swing * swing_speed,
        thickness_retention: e_thick,
        length_retention: e_len,
    }
}

/// Extra loss outside the center of the barrel for fixed-contact replays.
fn off_center_length_retention(length_offset_m: f64, max_loss: f64) -> f64 {
    let t = ((length_offset_m.abs() - 0.02) / 0.08).clamp(0.0, 1.0);
    1.0 - max_loss * t * t * (3.0 - 2.0 * t)
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

/// Fraction of the incoming spin vector added to collision-generated spin.
pub const PITCH_SPIN_RETENTION: f64 = 0.20;

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
    let pitch_retained_rate = pitch_spin_rate * PITCH_SPIN_RETENTION;

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

/// Historical coefficient retained for reference controls only.
pub const LIVE_FLIGHT_DRAG_PER_M: f64 = flight_drag::LEGACY_DRAG_PER_M;
pub const LIVE_FLIGHT_DT_SEC: f64 = 0.01;

/// Explicit flight controls. Normal gameplay uses the published drag default.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TrajectoryEnvironment {
    pub impact_height_m: f64,
    pub wind_speed_ms: f64,
    pub wind_direction_deg: f64,
    pub stop_at_first_ground: bool,
    pub drag_model: FlightDragModel,
    /// None: the legacy scalar spin model assumes all spin is transverse.
    pub total_spin_rpm: Option<f64>,
}
impl TrajectoryEnvironment {
    pub fn for_stadium(stadium: &Stadium) -> Self {
        let wind = stadium.default_wind();
        Self {
            impact_height_m: 0.90,
            wind_speed_ms: wind.speed_m_per_s,
            wind_direction_deg: wind.dir_deg,
            stop_at_first_ground: false,
            drag_model: flight_drag::current_model(),
            total_spin_rpm: None,
        }
    }
}
pub fn calculate_trajectory(
    launch_speed_ms: f64, // Batted ball exit velocity (m/s)
    vla_deg: f64,         // Vertical launch angle VLA (deg)
    hla_deg: f64,         // Horizontal launch angle HLA (deg) (+: right/opposite, -: left/pull)
    spin_rate_rpm: f64,   // Spin rate (rpm)
    spin_angle_deg: f64,  // Spin angle (deg) (0: backspin, 90: slider spin, 270: screw spin)
    stadium: &Stadium,
) -> Result<BattedBall, GameError> {
    calculate_trajectory_with_environment(
        launch_speed_ms,
        vla_deg,
        hla_deg,
        spin_rate_rpm,
        spin_angle_deg,
        stadium,
        TrajectoryEnvironment::for_stadium(stadium),
    )
}

pub fn calculate_trajectory_with_environment(
    launch_speed_ms: f64,
    vla_deg: f64,
    hla_deg: f64,
    spin_rate_rpm: f64,
    spin_angle_deg: f64,
    stadium: &Stadium,
    environment: TrajectoryEnvironment,
) -> Result<BattedBall, GameError> {
    integrate_trajectory(
        launch_speed_ms,
        vla_deg,
        hla_deg,
        spin_rate_rpm,
        spin_angle_deg,
        stadium,
        environment,
        GROUND_FRICTION,
        None::<fn(GrounderFlightSample)>,
        None,
    )
}

const GROUND_FRICTION: f64 = 0.25;

#[derive(Clone, Copy)]
pub(crate) struct GrounderFlightSample {
    pub time_sec: f64,
    pub position: PolarPosition,
    pub height_m: f64,
    pub horizontal_speed_ms: f64,
    pub after_bounce: bool,
}

/// Replay the exact live integrator with fixed launch inputs and a different ground loss.
pub(crate) fn replay_grounder_flight(
    ball: &BattedBall,
    hla_deg: f64,
    stadium: &Stadium,
    friction_scale: f64,
    observe: impl FnMut(GrounderFlightSample),
) -> Result<BattedBall, GameError> {
    integrate_trajectory(
        ball.launch_speed,
        ball.launch_angle,
        hla_deg,
        ball.spin_rate,
        ball.spin_angle,
        stadium,
        TrajectoryEnvironment::for_stadium(stadium),
        GROUND_FRICTION * friction_scale,
        Some(observe),
        None,
    )
}

#[derive(Serialize)]
pub(crate) struct FairFenceEvent {
    pub time_sec: f64,
    pub position: PolarPosition,
    pub ball_height_m: f64,
    pub fence_height_m: f64,
    pub height_above_fence_m: f64,
    pub bounce_count: u32,
}

/// Replay the live launch with current flight settings and collect fair fence decisions.
pub(crate) fn replay_fence_flight(
    ball: &BattedBall,
    hla_deg: f64,
    stadium: &Stadium,
    observe: impl FnMut(GrounderFlightSample),
) -> Result<Vec<FairFenceEvent>, GameError> {
    let mut events = Vec::new();
    integrate_trajectory(
        ball.launch_speed,
        ball.launch_angle,
        hla_deg,
        ball.spin_rate,
        ball.spin_angle,
        stadium,
        TrajectoryEnvironment::for_stadium(stadium),
        GROUND_FRICTION,
        Some(observe),
        Some(&mut events),
    )?;
    Ok(events)
}

fn integrate_trajectory(
    launch_speed_ms: f64,
    vla_deg: f64,
    hla_deg: f64,
    spin_rate_rpm: f64,
    spin_angle_deg: f64,
    stadium: &Stadium,
    environment: TrajectoryEnvironment,
    ground_friction: f64,
    mut observe: Option<impl FnMut(GrounderFlightSample)>,
    mut fair_fence_events: Option<&mut Vec<FairFenceEvent>>,
) -> Result<BattedBall, GameError> {
    if ![
        environment.impact_height_m,
        environment.wind_speed_ms,
        environment.wind_direction_deg,
    ]
    .iter()
    .all(|x| x.is_finite())
        || environment.impact_height_m <= 0.0
        || environment.wind_speed_ms < 0.0
        || !spin_rate_rpm.is_finite()
        || spin_rate_rpm < 0.0
        || environment
            .total_spin_rpm
            .is_some_and(|rpm| !rpm.is_finite() || rpm < 0.0 || rpm + 1e-8 < spin_rate_rpm)
    {
        return Err(GameError::InvalidTrajectoryEnvironment);
    }
    let vla_rad = vla_deg.to_radians();
    let hla_rad = hla_deg.to_radians();

    // Initial velocity vector
    let mut v_x = launch_speed_ms * vla_rad.cos() * hla_rad.sin();
    // Preserve the sign: |HLA| > 90 degrees travels behind home plate.
    let mut v_y = launch_speed_ms * vla_rad.cos() * hla_rad.cos();
    let mut v_z = launch_speed_ms * vla_rad.sin();

    // Contact height (impact position above ground: e.g. 0.9m)
    let impact_height_m = environment.impact_height_m;

    // Initial position
    let mut pos_x: f64 = 0.0;
    let mut pos_y: f64 = 0.0;
    let mut pos_z = impact_height_m;
    let mut max_height_m = pos_z; // Tracks the max height reached (initial value: impact height)

    // Current spin state for calculation (decays and changes after bounces)
    let mut current_spin_rate = spin_rate_rpm;
    let mut current_total_spin_rate = environment.total_spin_rpm.unwrap_or(spin_rate_rpm);
    let mut current_spin_angle = spin_angle_deg;

    // Decompose the wind vector
    let wind_rad = environment.wind_direction_deg.to_radians();
    let wind_v_x = environment.wind_speed_ms * wind_rad.sin();
    let wind_v_y = environment.wind_speed_ms * wind_rad.cos();

    let mut current_time = 0.0;
    let dt = LIVE_FLIGHT_DT_SEC; // 10ms steps
    let mut bounce_count = 0;

    let mut first_bounce_position = None;
    let mut first_bounce_time_sec = None;
    let mut fence_impact_position = None;
    let mut fence_impact_time_sec = None;
    let mut result_type = OutboundResult::InField;

    const RESTITUTION_COEFF: f64 = 0.45; // Ground restitution coefficient
    const WALL_RESTITUTION: f64 = 0.60; // Fence restitution coefficient

    loop {
        let prev_x = pos_x;
        let prev_y = pos_y;

        // --- Physics update (air resistance, Magnus, velocity, position) ---
        let rel_v_x = v_x - wind_v_x;
        let rel_v_y = v_y - wind_v_y;
        let rel_v_z = v_z;
        let rel_speed = (rel_v_x.powi(2) + rel_v_y.powi(2) + rel_v_z.powi(2)).sqrt();

        let drag_coefficient =
            flight_drag::drag_per_m(environment.drag_model, current_total_spin_rate);
        let drag_a_x = -drag_coefficient * rel_speed * rel_v_x;
        let drag_a_y = -drag_coefficient * rel_speed * rel_v_y;
        let drag_a_z = -drag_coefficient * rel_speed * rel_v_z;

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
            if pos_z > 0.0 {
                if let Some(events) = &mut fair_fence_events {
                    events.push(FairFenceEvent {
                        time_sec: current_time,
                        position: PolarPosition::new(fence_distance, current_spray_angle_deg),
                        ball_height_m: pos_z,
                        fence_height_m: stadium.fence_height,
                        height_above_fence_m: pos_z - stadium.fence_height,
                        bounce_count,
                    });
                }
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
        let mut stopped = false;
        if pos_z <= 0.0 {
            pos_z = 0.0;
            bounce_count += 1;
            if bounce_count == 1 {
                first_bounce_position = Some(PolarPosition::new(
                    current_distance_m,
                    current_spray_angle_deg,
                ));
                first_bounce_time_sec = Some(current_time);
                if environment.stop_at_first_ground {
                    break;
                }
            }

            v_z = -v_z * RESTITUTION_COEFF;
            v_x *= 1.0 - ground_friction;
            v_y *= 1.0 - ground_friction;

            // NOTE: After the bounce, ground friction turns the spin into topspin (180°) and the spin rate decays
            current_spin_rate *= 0.5;
            current_total_spin_rate *= 0.5;
            current_spin_angle = 180.0;

            if v_z.abs() < 0.5 {
                v_z = 0.0;
                let horiz_speed = (v_x.powi(2) + v_y.powi(2)).sqrt();
                if horiz_speed < 0.2 {
                    stopped = true;
                }
            }
        }

        if let Some(observe) = &mut observe {
            observe(GrounderFlightSample {
                time_sec: current_time,
                position: PolarPosition::new(pos_x.hypot(pos_y), pos_x.atan2(pos_y).to_degrees()),
                height_m: pos_z,
                horizontal_speed_ms: v_x.hypot(v_y),
                after_bounce: bounce_count > 0,
            });
        }

        if stopped {
            break;
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

    calculate_batted_ball_from_speed(batter, ball, contact, angles, stadium, launch_speed_ms)
}

pub(crate) fn calculate_batted_ball_from_speed(
    batter: &BatterInfo,
    ball: PitchedBall,
    contact: &SwingContactResult,
    angles: &BattedBallAngles,
    stadium: &Stadium,
    launch_speed_ms: f64,
) -> Result<BattedBall, GameError> {
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
    let (ball, metrics, _) =
        calculate_batted_ball_with_length_loss(rng, batter, ball, contact, stadium, 0.0)?;
    Ok((ball, metrics))
}

pub(crate) fn calculate_batted_ball_with_length_loss(
    rng: &mut dyn RandomProvider,
    batter: &BatterInfo,
    ball: PitchedBall,
    contact: &SwingContactResult,
    stadium: &Stadium,
    max_loss: f64,
) -> Result<(BattedBall, PlayerGameBattingMetrics, f64), GameError> {
    let angles = calculate_launch_angles(rng, contact, batter)?;
    let depth = if CONTACT_GEOMETRY_SCALE.get() == CONTACT_TIMING_POSITION_SCALE {
        contact.timing_impact_x_m
    } else {
        batter.swing_speed * contact.timing_offset_sec * CONTACT_TIMING_POSITION_SCALE
    };
    let mut terms = launch_speed_terms(
        ball.speed,
        batter.swing_speed,
        batter.swing_power,
        depth,
        contact.thickness_offset_m,
        contact.length_offset_m,
    );
    if max_loss != 0.0 {
        terms.length_retention *= off_center_length_retention(contact.length_offset_m, max_loss);
    }
    let retention = terms.thickness_retention * terms.length_retention;
    let batted_ball = calculate_batted_ball_from_speed(
        batter,
        ball,
        contact,
        &angles,
        stadium,
        terms.exit_speed_ms(),
    )?;
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

    Ok((batted_ball, metrics, retention))
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
    fn collision_power_trial_interpolates_the_coefficient_at_each_depth() {
        for depth in [-0.10, 0.0, 0.10] {
            for power in [-1.0, 2.0] {
                let terms = |power| launch_speed_terms(40.0, 35.0, power, depth, 0.03, 0.05);
                let original = terms(power);
                let median = terms(0.1);
                let uniform = with_collision_power_trial(0.1, 0.0, || terms(power));
                let half = with_collision_power_trial(0.1, 0.5, || terms(power));
                assert_eq!(uniform.exit_speed_ms(), median.exit_speed_ms());
                assert_eq!(half.pitch_term_ms, original.pitch_term_ms);
                assert_eq!(half.thickness_retention, original.thickness_retention);
                assert_eq!(half.length_retention, original.length_retention);
                let midpoint = original.swing_term_ms.midpoint(median.swing_term_ms);
                assert!((half.swing_term_ms - midpoint).abs() < 1e-12);
                assert_eq!(terms(power).exit_speed_ms(), original.exit_speed_ms());
            }
        }
    }

    #[test]
    fn fast_swing_trial_widens_only_execution_errors_without_new_draws() {
        use crate::domain::random_provider::RealRng;

        let location = BallLocation { x: 0.0, y: 0.0 };
        with_swing_execution_model(SwingExecutionModel::Mixture, || {
            for (speed, expected) in [(33.5, 1.0), (34.0, 1.0), (34.75, 1.25), (35.5, 1.5)] {
                let mut hitter = batter(RL::Right);
                hitter.swing_speed = speed;
                let mut control_rng = RealRng::from_seed(42);
                let mut trial_rng = RealRng::from_seed(42);
                let control = calculate_swing_execution_error(&mut control_rng, &hitter, &location);
                let trial = with_fast_swing_error_trial(1.5, || {
                    calculate_swing_execution_error(&mut trial_rng, &hitter, &location)
                });
                assert_eq!(trial.actual_bat_angle_deg, control.actual_bat_angle_deg);
                assert_eq!(trial.actual_attack_angle_deg, control.actual_attack_angle_deg);
                for (actual, base) in [
                    (trial.additional_timing_sec, control.additional_timing_sec),
                    (trial.additional_x_m, control.additional_x_m),
                    (trial.additional_z_m, control.additional_z_m),
                ] {
                    assert!((actual - base * expected).abs() < 1e-12);
                }
                assert_eq!(control_rng.random(), trial_rng.random());
            }
        });
    }

    #[test]
    fn extra_length_loss_preserves_center_and_increases_smoothly_off_center() {
        for loss in [0.0, 0.20, 0.35] {
            assert_eq!(off_center_length_retention(0.0, loss), 1.0);
            assert_eq!(off_center_length_retention(-0.02, loss), 1.0);
            assert_eq!(off_center_length_retention(0.10, loss), 1.0 - loss);
            assert_eq!(off_center_length_retention(-0.14, loss), 1.0 - loss);
            let mut previous = 1.0;
            for mm in 0..=120 {
                let retention = off_center_length_retention(f64::from(mm) / 1000.0, loss);
                assert!(retention <= previous);
                previous = retention;
            }
        }
    }


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
    fn execution_timing_reaches_contact_without_overwriting_pitch_prediction_error() {
        let batter = batter(RL::Right);
        let offset = PitchDisplacement {
            timing_offset_sec: -0.001,
            ..Default::default()
        };
        let error = SwingExecutionError {
            additional_timing_sec: 0.003,
            ..Default::default()
        };
        let contact = evaluate_swing_contact(&mut FixedRng::new(0.5), &batter, &offset, &error);
        let metrics = PlayerGameBattingMetrics::from_contact(&contact);
        assert!((metrics.timing_error - 0.002).abs() < 1e-12);
        assert!((metrics.adapted_timing_offset_sec.unwrap() + 0.001).abs() < 1e-12);
        let expected_x = 0.002 * batter.swing_speed * CONTACT_TIMING_POSITION_SCALE;
        assert!((contact.offset_x_m - expected_x).abs() < 1e-12);
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

    #[test]
    fn protection_changes_local_contact_geometry_without_changing_length() {
        let batter = batter(RL::Right);
        let angle: f64 = 30.0;
        let (sin, cos) = angle.to_radians().sin_cos();
        for sign in [-1.0, 1.0] {
            for (thickness, before, after, expected) in [
                (0.020, SwingContactType::SolidContact, SwingContactType::WeakContact, 0.0312),
                (0.074, SwingContactType::SwungAndMiss, SwingContactType::MarginalContact, 0.0644),
                (0.100, SwingContactType::SwungAndMiss, SwingContactType::SwungAndMiss, 0.100),
            ] {
                let displacement = PitchDisplacement {
                    crossfire_multiplier: 1.0,
                    release_x_factor: 1.0,
                    horizontal_offset_m: 0.04 * cos - sign * thickness * sin,
                    vertical_offset_m: 0.04 * sin + sign * thickness * cos,
                    timing_offset_sec: 0.0,
                };
                let mut error = SwingExecutionError {
                    actual_bat_angle_deg: angle,
                    ..Default::default()
                };
                let control = evaluate_swing_contact(&mut FixedRng::new(0.5), &batter, &displacement, &error);
                assert_eq!(control.original_contact_type, before);
                apply_contact_protection_trial(&batter, &displacement, &mut error, ContactProtectionTrial::Combined);
                let trial = evaluate_swing_contact(&mut FixedRng::new(0.5), &batter, &displacement, &error);
                assert_eq!(trial.original_contact_type, after);
                assert!((trial.length_offset_m - 0.04).abs() < 1e-12);
                assert!((trial.thickness_offset_m - expected).abs() < 1e-12);
                assert_eq!(error.actual_bat_angle_deg, angle);
            }
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
    fn full_game_geometry_scope_keeps_default_exit_speed_depth() -> TestResult {
        let batter = batter(RL::Right);
        let stadium = Stadium::default();
        let pitch = PitchedBall {
            pitch_type: PitchType::FourSeamFastball,
            speed: 41.67,
            spin_rate: 2300.0,
            spin_angle: 0.0,
            spin_efficiency: 0.95,
            release_point: Vector3D { x: 1.6, y: 16.74, z: 1.7 },
            flight_time: 0.42,
            aim_zone: TargetZone::Center,
            aim_location: BallLocation { x: 0.0, y: 0.0 },
            actual_location: BallLocation { x: 0.0, y: 0.0 },
        };
        let offset = PitchDisplacement {
            timing_offset_sec: 0.003,
            ..Default::default()
        };
        let execution = SwingExecutionError {
            actual_attack_angle_deg: 10.0,
            ..Default::default()
        };
        let contact = || {
            evaluate_swing_contact(&mut FixedRng::new(0.5), &batter, &offset, &execution)
        };
        let baseline = contact();
        assert_eq!(
            pre_trial_exit_speed_ms(&batter, pitch.speed, &offset, &execution),
            Some(calculate_launch_speed_with_power(
                &baseline, pitch.speed, batter.swing_speed, batter.swing_power,
            )),
        );
        let miss = PitchDisplacement {
            horizontal_offset_m: 1.0,
            ..offset
        };
        assert_eq!(pre_trial_exit_speed_ms(&batter, pitch.speed, &miss, &execution), None);
        with_contact_geometry_scale(1.0, || -> TestResult {
            let trial = contact();
            assert_eq!(trial.timing_position_scale, Some(1.0));
            assert_ne!(trial.length_offset_m, baseline.length_offset_m);
            let (ball, _) = calculate_batted_ball_with_metrics(
                &mut FixedRng::new(0.5), &batter, pitch, &trial, &stadium,
            )?;
            let expected = calculate_launch_speed_with_depth(
                &trial, pitch.speed, batter.swing_speed, batter.swing_power,
                baseline.timing_impact_x_m,
            );
            assert_eq!(ball.launch_speed, expected);
            assert_eq!(
                pre_trial_exit_speed_ms(&batter, pitch.speed, &offset, &execution),
                Some(ball.launch_speed),
            );
            assert_ne!(
                ball.launch_speed,
                calculate_launch_speed_with_power(&trial, pitch.speed, batter.swing_speed, batter.swing_power)
            );
            Ok(())
        })?;
        assert_eq!(contact().timing_position_scale, Some(CONTACT_TIMING_POSITION_SCALE));
        Ok(())
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

    #[test]
    fn edge_take_trial_is_count_sensitive_and_local_to_the_boundary() {
        let decision = |count, x, y| {
            let location = BallLocation { x, y };
            let mut baseline = calculate_swing_decision(
                PlateApproach::Aggressive,
                count,
                PitchType::FourSeamFastball,
                &factor(location.distance_from_zone_edge()),
            );
            let original = baseline;
            baseline.apply_edge_take_trial(location);
            (original, baseline)
        };
        for (count, expected) in [
            (CountStatus::C31, 0.80),
            (CountStatus::C20, 0.65),
            (CountStatus::C11, 0.45),
            (CountStatus::C01, 0.35),
            (CountStatus::C12, 0.20),
        ] {
            let (before, after) = decision(count, -1.0, 0.6);
            assert!((before.score - after.score - expected).abs() < 1e-12);
            assert!((before.zone_factor - after.zone_factor - expected).abs() < 1e-12);
            assert_eq!(before.protection_factor, after.protection_factor);
            assert_eq!(after.probability, sigmoid(after.score));
            assert!(after.probability < before.probability);
            for (x, y) in [(0.79, 0.0), (1.6, 0.0)] {
                let (before, after) = decision(count, x, y);
                assert_eq!(before.probability, after.probability);
            }
            let probabilities = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.6]
                .map(|y| decision(count, 0.0, y).1.probability);
            assert!(probabilities.windows(2).all(|pair| pair[0] >= pair[1]));
        }
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

#[cfg(test)]
mod flight_environment_tests {
    use super::*;
    use serde_json::Value;
    fn close(a: &Value, b: &Value) {
        match (a, b) {
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(a.len(), b.len());
                for (k, v) in a {
                    close(v, &b[k]);
                }
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len());
                for (a, b) in a.iter().zip(b) {
                    close(a, b);
                }
            }
            (Value::Number(a), Value::Number(b)) => {
                assert!((a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-8)
            }
            _ => assert_eq!(a, b),
        }
    }
    #[test]
    fn historical_trajectories_are_preserved_in_legacy_scope() {
        let v: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/flight_legacy_contract.json"
        ))
        .unwrap();
        for row in v["cases"].as_array().unwrap() {
            let p = row["input"].as_array().unwrap();
            let n = |i: usize| p[i].as_f64().unwrap();
            let ball = flight_drag::with_model(FlightDragModel::Legacy, || {
                calculate_trajectory(n(0), n(1), n(2), n(3), n(4), &Stadium::default())
            })
            .unwrap();
            close(&row["output"], &serde_json::to_value(ball).unwrap());
        }
    }
    #[test]
    fn first_ground_mode_uses_explicit_height_and_no_wind() {
        let s = Stadium::new(1, "wide".into(), 500.0, 500.0, 2.0);
        let e = TrajectoryEnvironment {
            impact_height_m: 0.9,
            wind_speed_ms: 0.0,
            wind_direction_deg: 0.0,
            stop_at_first_ground: true,
            ..TrajectoryEnvironment::for_stadium(&s)
        };
        let ball = calculate_trajectory_with_environment(45.0, 20.0, 0.0, 0.0, 0.0, &s, e).unwrap();
        assert_eq!(ball.total_time, ball.first_bounce_time.unwrap());
        assert_eq!(ball.final_position, ball.first_bounce_position.unwrap());
        let invalid = TrajectoryEnvironment {
            impact_height_m: -0.1,
            ..e
        };
        assert!(matches!(
            calculate_trajectory_with_environment(45.0, 20.0, 0.0, 0.0, 0.0, &s, invalid),
            Err(GameError::InvalidTrajectoryEnvironment)
        ));
    }
}

#[cfg(test)]
mod published_drag_tests {
    use super::*;
    use crate::domain::shared::flight_reference::{self, ForceModel, Launch, Settings};
    #[test]
    fn default_flight_matches_independent_drag_only_control() {
        let stadium = Stadium::new(1, "wide".into(), 500.0, 500.0, 2.0);
        for speed in [40.2336, 45.0, 49.1744] {
            for vla in [20.0, 30.0, 35.0] {
                for rpm in [0.0, 400.0, 2600.0, 4000.0] {
                    let environment = TrajectoryEnvironment {
                        wind_speed_ms: 0.0,
                        stop_at_first_ground: true,
                        ..TrajectoryEnvironment::for_stadium(&stadium)
                    };
                    let ball = calculate_trajectory_with_environment(
                        speed,
                        vla,
                        0.0,
                        rpm,
                        0.0,
                        &stadium,
                        environment,
                    )
                    .unwrap();
                    let reference = flight_reference::integrate(
                        Launch {
                            speed_ms: speed,
                            vla_deg: vla,
                            hla_deg: 0.0,
                            spin_rpm: rpm,
                            spin_angle_deg: 0.0,
                            height_m: 0.9,
                            wind_speed_ms: 0.0,
                            wind_direction_deg: 0.0,
                        },
                        Settings::default(),
                        ForceModel::PublishedDragLiveLift,
                    )
                    .unwrap();
                    assert!(
                        (ball.first_bounce_position.unwrap().distance - reference.carry_m).abs()
                            < 0.8
                    );
                    assert!(
                        (ball.first_bounce_time.unwrap() - reference.landing_time_sec).abs() < 0.05
                    );
                }
            }
        }
    }
    #[test]
    fn gyro_spin_affects_drag_without_inventing_lift() {
        let stadium = Stadium::new(1, "wide".into(), 500.0, 500.0, 2.0);
        let environment = TrajectoryEnvironment {
            wind_speed_ms: 0.0,
            stop_at_first_ground: true,
            ..TrajectoryEnvironment::for_stadium(&stadium)
        };
        let flight = |rpm| {
            calculate_trajectory_with_environment(
                45.0,
                20.0,
                0.0,
                0.0,
                0.0,
                &stadium,
                TrajectoryEnvironment {
                    total_spin_rpm: Some(rpm),
                    ..environment
                },
            )
            .unwrap()
        };
        let no_spin = flight(0.0);
        let gyro = flight(4000.0);
        assert_eq!(gyro.spin_rate, 0.0);
        assert!(
            gyro.first_bounce_position.unwrap().distance
                < no_spin.first_bounce_position.unwrap().distance
        );
        assert!(gyro.max_height < no_spin.max_height);
        for rpm in [-1.0, f64::NAN, f64::INFINITY] {
            assert!(
                calculate_trajectory_with_environment(
                    45.0,
                    20.0,
                    0.0,
                    0.0,
                    0.0,
                    &stadium,
                    TrajectoryEnvironment {
                        total_spin_rpm: Some(rpm),
                        ..environment
                    }
                )
                .is_err()
            );
        }
        assert!(
            calculate_trajectory_with_environment(
                45.0,
                20.0,
                0.0,
                1000.0,
                0.0,
                &stadium,
                TrajectoryEnvironment {
                    total_spin_rpm: Some(999.0),
                    ..environment
                }
            )
            .is_err()
        );
    }
}
