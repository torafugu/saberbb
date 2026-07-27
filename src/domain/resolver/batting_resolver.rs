use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::ball::{BattedBall, PitchedBall, TrajectoryType, CONVERT_FACTOR_KMH_TO_MS};
use crate::domain::shared::player::BatterInfo;
use crate::domain::util::GRAVITY;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use strum_macros::{AsRefStr, EnumIter, EnumString};

// Standard reference swing speed (km/h)
const REF_SWING_SPEED: f64 = 120.0;
// Maximum spin rate generated when fully brushing the ball at reference swing (rpm)
const MAX_COLLISION_SPIN_AT_REF_SPEED: f64 = 4000.0;
// Scaling: 2300rpm = 1.0 Magnus force
const MAGNUS_FORCE_BASE_SPIN_RATE: f64 = 2300.0;

// Mismatch between the batter's swing prediction and the actual pitch
#[derive(Clone, Debug)]
pub struct SwingContactResult {
    // NOTE: Spatial sweet-spot offset (0.0: perfectly centered ~ 1.0: completely missing the zone)
    pub vertical_offset: f64,
    pub horizontal_offset: f64,

    // NOTE: Timing offset (-1.0: way early ~ 0.0: just right ~ +1.0: way late)
    pub timing_offset: f64,
}
impl SwingContactResult {
    pub fn offset(&self) -> f64 {
        (self.vertical_offset.powi(2) + self.horizontal_offset.powi(2)).sqrt()
    }

    /// Determine whether the swing results in a miss
    pub fn is_swing_and_miss(&self) -> bool {
        // If either spatial or timing offset exceeds the threshold (or both combined), it's a miss
        (self.vertical_offset > 0.8 && self.horizontal_offset > 0.8) || self.timing_offset.abs() > 0.7
    }
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

pub fn evaluate_swing(
    _batter: &BatterInfo,
    ball: &PitchedBall,
    // Pass batter's target pitch and swing timing input as needed
) -> SwingContactResult {
    // TODO: Calculate spatial offset from pitch trajectory change (pitch type + spin) vs batter's coverage ability
    // TODO: Calculate timing offset from velocity/change of pace vs batter's swing timingA

    let pitch_displacement = calculate_pitch_displacement(ball);

    SwingContactResult {
        vertical_offset: pitch_displacement.vertical,
        horizontal_offset: pitch_displacement.horizontal,
        timing_offset: 0.0,
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, EnumIter, AsRefStr,
)]
#[strum(ascii_case_insensitive)]
pub enum FieldSector {
    FoulPull, // NOTE: Foul of Pull-side (right-handed batter → left field, left-handed batter → right field)
    Pull,      // NOTE: Pull (right-handed batter → left field, left-handed batter → right field)
    Center,    // NOTE: Center field
    Opposite, // NOTE: Opposite field (right-handed batter → right field, left-handed batter → left field)
    FoulOpposite, // NOTE: Foul of Opposite-side (right-handed batter → right field, left-handed batter → left field)
}

fn inner_choose_sector(rng: &mut dyn RandomProvider,  offset_factor_percent: f64, batter: &BatterInfo) -> FieldSector {
    let total_weight = batter.weight_pull
        + batter.weight_center
        + batter.weight_opposite
        + batter.weight_foul_pull
        + batter.weight_foul_opposite;
    let roll = rng.range_f64(0.0, total_weight);
    let mut modified_roll = roll + offset_factor_percent;

    if modified_roll < batter.weight_foul_pull {
        return FieldSector::FoulPull;
    }
    modified_roll -= batter.weight_foul_pull;

    if modified_roll < batter.weight_pull {
        return FieldSector::Pull;
    }
    modified_roll -= batter.weight_pull;

    if modified_roll < batter.weight_center {
        return FieldSector::Center;
    }
    modified_roll -= batter.weight_center;

    if modified_roll < batter.weight_opposite {
        return FieldSector::Opposite;
    }

    return FieldSector::FoulOpposite;
}

fn sample_launch_speed(rng: &mut dyn RandomProvider, ball_speed: f64, swing_speed: f64, spacial_offset: f64, timing_offset: f64) -> f64 {
    // Theoretical maximum exit velocity for a squared-up ball (V_max)
    let a = 1.00; // Swing efficiency
    let b = 0.20; // Rebound efficiency
    let v_max = (a * swing_speed) + (b * ball_speed);

    let launch_speed = v_max 
        * (1.0 - spacial_offset * 0.3)  // Off-center contact causes up to 30% reduction
        * (1.0 - timing_offset.abs() * 0.2); // Timing delay causes up to 20% reduction

    // Add the final variation with normally distributed noise (mean 0, standard deviation 5 km/h)
    // Cap the minimum value to prevent negative or excessively slow speeds (10 km/h)
    let final_speed = launch_speed + rng.normal_random(0.0, 5.0, 0.0, 1.0, 0.0);
    final_speed.max(10.0)
}

fn sample_spray_angle(rng: &mut dyn RandomProvider, timing_offset: f64, tendency: &BatterInfo) -> f64 {
    let offset_factor_percent = timing_offset * 25.0; // The range of offset_factor_percent is -25.0 % to 25.0%

    // Step 1: Decide the sector
    let chosen_sector = inner_choose_sector(rng, offset_factor_percent, tendency);

    // Step 2: Get the angle range for that sector
    let (min_angle, max_angle) = tendency.get_angle_range(chosen_sector);
    let min_angle = min_angle as f64;
    let max_angle = max_angle as f64;

    // Step 3: Randomly sample within the range
    let mean = (min_angle + max_angle) * 0.5;
    let std_dev = (max_angle - min_angle) / 6.0;
    // 10% of Timing offset effects to random skew.
    let final_angle = rng
        .normal_random(mean, std_dev, timing_offset * 0.1, 1.0, 0.0)
        .clamp(min_angle, max_angle);

    final_angle
}

pub fn sample_launch_angle(
    rng: &mut dyn RandomProvider,
    batter: &BatterInfo,
    contact: &SwingContactResult,
) -> f64 {
    // 1. Create normal distribution noise based on batter's contact accuracy (meet skill)
    // Mean 0.0, standard deviation consistency_sigma
    let vertical_noise = rng.normal_random(0.0, batter.consistency_sigma, 0.0, 1.0, 0.0);

    // 2. Add small noise to the sweet spot offset (vertical)
    let noisy_vertical = (contact.vertical_offset + vertical_noise).clamp(-1.0, 1.0);

    // 3. Calculate VLA (base launch angle - offset amount × 30°)
    let max_angle_deviation = 30.0;
    let base_vla = batter.base_launch_angle - (noisy_vertical * max_angle_deviation);

    // 4. Add slight air resistance/seam-induced variation to launch angle (e.g. Gaussian noise with std dev 1.5°)
    let final_vla = base_vla + rng.normal_random(0.0, 1.5, 0.0, 1.0, 0.0);

    final_vla.clamp(-15.0, 85.0)
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

fn calculate_collision_spin(ball: PitchedBall, swing_speed: f64, contact: &SwingContactResult) -> (f64, f64) {

    // 1. Calculate total distance from sweet spot using Pythagorean theorem
    let distance = (contact.horizontal_offset.powi(2) + contact.vertical_offset.powi(2)).sqrt().min(1.0);

    // 2. Calculate contact point angle (radians) using atan2(y, x)
    // y = vertical, x = horizontal
    let impact_angle_rad = contact.vertical_offset.atan2(contact.horizontal_offset);

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
    let (combined_spin_rate, combined_spin_angle) = combine_batted_spin(ball.spin_rate, ball.spin_angle, raw_spin_rate, spin_angle_deg);

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
    v: f64,              // Batted ball exit velocity (m/s)
    vla_deg: f64,        // Vertical launch angle VLA (deg)
    hla_deg: f64,        // Horizontal launch angle HLA (deg) (+: right/opposite, -: left/pull)
    spin_rate: f64,      // Spin rate (rpm)
    spin_angle_deg: f64,   // Spin angle (deg) (0: backspin, 90: slider spin, 270: screw spin)
) -> (f64, f64, f64) {
    let vla_rad = vla_deg.to_radians();
    let hla_rad = hla_deg.to_radians();
    let spin_angle_rad = spin_angle_deg.to_radians();

    // 1. Decompose Magnus acceleration (vertical vs horizontal)
    // Total Magnus force (approx. 3.5 m/s² at 2500rpm, 40m/s)
    let total_magnus_accel = (spin_rate / 2500.0) * (v / 40.0) * 3.5;

    // Decompose into vertical lift (cos) and horizontal break (sin)
    let vertical_magnus = total_magnus_accel * spin_angle_rad.cos();
    let side_magnus = total_magnus_accel * spin_angle_rad.sin();

    // 2. Calculate hang time and depth distance (Y-axis)
    let g_eff = (GRAVITY - vertical_magnus).max(3.0); // Effective gravity
    let flight_time = (2.0 * v * vla_rad.sin()) / g_eff;

    // Horizontal initial velocity component
    let v_horizontal = v * vla_rad.cos();

    // Air resistance correction (simplified model)
    let drag_factor = (1.0 - (0.005 * v) - (0.0001 * spin_rate)).clamp(0.5, 0.95);

    // Y-axis flight distance (linear distance × cos(HLA) × air resistance)
    let distance = (v_horizontal * hla_rad.cos() * flight_time) * drag_factor;

    // 3. Calculate horizontal arrival angle
    // A. Horizontal break from side spin during flight (1/2 * a * t²)
    let x_from_side_spin = 0.5 * side_magnus * flight_time.powi(2);

    // B. Add side-spin deflection angle to initial launch angle (hla_deg)
    // atan2(lateral deviation, depth distance) gives additional deflection angle (rad)
    let spin_curve_angle_rad = x_from_side_spin.atan2(distance);
    let spin_curve_angle_deg = spin_curve_angle_rad.to_degrees();

    // Final horizontal arrival angle (polar coordinate angle θ)
    let final_spray_angle_deg = hla_deg + spin_curve_angle_deg;
    
    (flight_time, distance, final_spray_angle_deg)
}

pub fn calculate_batted_ball(
    rng: &mut dyn RandomProvider,
    batter: &BatterInfo,
    ball: PitchedBall,
    contact: &SwingContactResult,
) -> BattedBall {
    

    // 1. Calculate exit velocity (damped by spatial offset & timing delay)
    let launch_speed = sample_launch_speed(rng, ball.speed, batter.swing_speed, contact.offset(), contact.timing_offset);

    // 2. Calculate vertical launch angle (VLA)
    // Vertical spatial offset (hitting above or below the ball) is the main factor
    let launch_angle = sample_launch_angle(rng, &batter, contact);

    // 3. Calculate batted ball spin
    // Inherit a small portion of the residual spin from pitch.spin_rate / pitch.spin_angle
    let (batted_spin_rate, batted_spin_angle)= calculate_collision_spin(ball, batter.swing_speed, contact);
    let trajectory = classify_trajectory_type(launch_angle, batted_spin_rate, batted_spin_angle);

    // 4. Calculate horizontal launch angle (HLA)
    let  hla_deg = sample_spray_angle(rng, contact.timing_offset, batter);

    let (hang_time, distance, spray_angle) = calculate_3d_flight_path(launch_speed * CONVERT_FACTOR_KMH_TO_MS, launch_angle, hla_deg, batted_spin_rate, batted_spin_angle);

    BattedBall::new(
        launch_speed,
        launch_angle,
        spray_angle,
        distance,
        hang_time,
        trajectory,
    )
}

#[cfg(test)]
mod tests {
    use crate::domain::random_provider::FixedRng;
    use crate::domain::resolver::batting_resolver::{
        BatterInfo, FieldSector, SwingContactResult, calculate_batted_ball, inner_choose_sector,
        sample_spray_angle,
    };
    use crate::domain::shared::ball::{PitchedBall, TrajectoryType};
    use crate::domain::shared::player::RL;
    use crate::domain::util::Vector3D;

    fn batter_with_weights(
        batting_side: RL,
        weight_pull: f64,
        weight_center: f64,
        weight_opposite: f64,
        weight_foul_left: f64,
        weight_foul_right: f64,
    ) -> BatterInfo {
        BatterInfo {
            batting_side,
            swing_speed: 150.0,
            base_launch_angle: 28.0,
            consistency_sigma: 0.03,
            weight_pull,
            weight_center,
            weight_opposite,
            weight_foul_pull: weight_foul_left,
            weight_foul_opposite: weight_foul_right,
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
            vertical_offset: 0.0,
            horizontal_offset: 0.0,
            timing_offset: 0.0,
        }
    }

    #[test]
    fn batter_get_angle_range_maps_pull_and_opposite_by_batting_side() {
        let right_hitter = batter_with_weights(RL::Right, 1.0, 0.0, 0.0, 0.0, 0.0);
        let left_hitter = batter_with_weights(RL::Left, 1.0, 0.0, 0.0, 0.0, 0.0);

        assert_eq!(
            right_hitter.get_angle_range(FieldSector::Pull),
            (-45.0, -15.0)
        );
        assert_eq!(
            right_hitter.get_angle_range(FieldSector::Opposite),
            (15.0, 45.0)
        );
        assert_eq!(left_hitter.get_angle_range(FieldSector::Pull), (15.0, 45.0));
        assert_eq!(
            left_hitter.get_angle_range(FieldSector::Opposite),
            (-45.0, -15.0)
        );
    }

    #[test]
    fn inner_choose_sector_returns_the_only_weighted_sector() {
        let cases = [
            (
                batter_with_weights(RL::Right, 1.0, 0.0, 0.0, 0.0, 0.0),
                FieldSector::Pull,
            ),
            (
                batter_with_weights(RL::Right, 0.0, 1.0, 0.0, 0.0, 0.0),
                FieldSector::Center,
            ),
            (
                batter_with_weights(RL::Right, 0.0, 0.0, 1.0, 0.0, 0.0),
                FieldSector::Opposite,
            ),
            (
                batter_with_weights(RL::Right, 0.0, 0.0, 0.0, 1.0, 0.0),
                FieldSector::FoulPull,
            ),
            (
                batter_with_weights(RL::Right, 0.0, 0.0, 0.0, 0.0, 1.0),
                FieldSector::FoulOpposite,
            ),
        ];

        for (batter, expected_sector) in cases {
            let mut rng = FixedRng::new(0.5);
            assert_eq!(inner_choose_sector(&mut rng, 0.0, &batter), expected_sector);
        }
    }

    #[test]
    fn sample_spray_angle_stays_inside_forced_sector_range() {
        let cases = [
            (
                batter_with_weights(RL::Right, 1.0, 0.0, 0.0, 0.0, 0.0),
                -45.0,
                -15.0,
            ),
            (
                batter_with_weights(RL::Right, 0.0, 1.0, 0.0, 0.0, 0.0),
                -15.0,
                15.0,
            ),
            (
                batter_with_weights(RL::Right, 0.0, 0.0, 1.0, 0.0, 0.0),
                15.0,
                45.0,
            ),
            (
                batter_with_weights(RL::Left, 1.0, 0.0, 0.0, 0.0, 0.0),
                15.0,
                45.0,
            ),
            (
                batter_with_weights(RL::Left, 0.0, 0.0, 1.0, 0.0, 0.0),
                -45.0,
                -15.0,
            ),
            (
                batter_with_weights(RL::Left, 0.0, 0.0, 0.0, 1.0, 0.0),
                45.0,
                90.0,
            ),
            (
                batter_with_weights(RL::Left, 0.0, 0.0, 0.0, 0.0, 1.0),
                -90.0,
                -45.0,
            ),
        ];

        for (batter, min_angle, max_angle) in cases {
            for _ in 0..20 {
                let mut rng = FixedRng::new(0.5);
                assert_between(sample_spray_angle(&mut rng, 0.0, &batter), min_angle, max_angle);
            }
        }
    }

    #[test]
    fn calculate_batted_ball_sets_physical_values_and_trajectory_specific_launch_angle() {
        let right_pull_hitter = batter_with_weights(RL::Right, 1.0, 0.0, 0.0, 0.0, 0.0);

        for _ in 0..50 {
            let mut rng = FixedRng::new(0.5);
            let ball = calculate_batted_ball(
                &mut rng,
                &right_pull_hitter,
                PitchedBall {
                    speed: 150.0,
                    spin_rate: 2300.0,
                    spin_angle: 0.0,
                    release_point: Vector3D {
                        x: 1.6,
                        y: 16.74,
                        z: 1.7
                    },
                },
                &centered_contact(),
            );

            assert!(ball.launch_speed_kmh >= 30.0);
            assert!(ball.distance().is_finite());
            assert!(ball.hang_time.is_finite());
            assert_between(ball.angle(), -45.0, -15.0);

            match ball.trajectory {
                TrajectoryType::Grounder => assert_between(ball.launch_angle, 0.0, 10.0),
                TrajectoryType::Liner => assert_between(ball.launch_angle, 10.0, 25.0),
                TrajectoryType::Fly => assert_between(ball.launch_angle, 25.0, 50.0),
                TrajectoryType::PopUp => assert_between(ball.launch_angle, 50.0, 80.0),
            }
        }
    }
}
