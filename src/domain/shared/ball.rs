use crate::domain::shared::game::PitchResult;
use crate::domain::shared::player::{PitchType, Position};
use crate::domain::strategy::pitch_call::TargetZone;
use crate::domain::util::{GRAVITY, PolarPosition, Vector3D};
use crate::t;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use std::fmt;
use strum_macros::{AsRefStr, EnumString};

const FOUL_DEGREE: f64 = 45.0;
const INFIELD_DISTANCE: f64 = 50.0;
const SHALLOW_DISTANCE: f64 = 45.0;

// Scaling coefficient calibrated so that at 2500rpm, 150km/h (41.67m/s), efficiency 1.0, acceleration is approx. 3.5 m/s²
// Coefficient K ≈ 3.5 / (2500.0 * 41.67) ≈ 0.0000336
const MAGNUS_COEFF: f64 = 0.0000336;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, AsRefStr, Serialize, Deserialize)]
pub enum TrajectoryType {
    Grounder,
    Liner,
    Fly,
    PopUp,
}
impl fmt::Display for TrajectoryType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TrajectoryType::Grounder => write!(f, "{}", t!("grounder")),
            TrajectoryType::Liner => write!(f, "{}", t!("liner")),
            TrajectoryType::Fly => write!(f, "{}", t!("fly")),
            TrajectoryType::PopUp => write!(f, "{}", t!("popup")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FieldedBall {
    pub ball: BattedBall,
    pub fielded_by: Position,
    pub time_to_field: f64,
    pub is_fly_catch: bool,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct BattedBall {
    pub launch_speed: f64,
    pub launch_angle: f64,
    pub polar_position: PolarPosition,
    pub hang_time: f64, // second
    pub trajectory: TrajectoryType,
}

impl BattedBall {
    pub fn new(
        launch_speed: f64,
        launch_angle: f64,
        spray_angle: f64,
        distance: f64,
        hang_time: f64,
        trajectory: TrajectoryType,
    ) -> Self {
        Self {
            launch_speed,
            launch_angle: launch_angle,
            polar_position: PolarPosition::new(distance, spray_angle),
            hang_time: hang_time,
            trajectory: trajectory,
        }
    }

    pub fn distance(&self) -> f64 {
        self.polar_position.distance
    }

    pub fn angle(&self) -> f64 {
        self.polar_position.angle
    }

    pub fn azimuth(&self) -> f64 {
        self.launch_angle.to_radians()
    }

    pub fn x(&self) -> f64 {
        self.polar_position.x
    }

    pub fn y(&self) -> f64 {
        self.polar_position.y
    }

    pub fn is_foul(&self) -> bool {
        if self.polar_position.angle.abs() >= FOUL_DEGREE {
            true
        } else {
            false
        }
    }

    pub fn is_infield(&self) -> bool {
        self.distance() < INFIELD_DISTANCE
    }

    pub fn is_shallow(&self) -> bool {
        self.distance() < SHALLOW_DISTANCE
    }

    pub fn calculate_height_at_distance(
        &self,
        target_distance: f64, // Distance at which to calculate height (m)
    ) -> f64 {
        let theta = self.launch_angle.to_radians();

        // 1. Apply drag coefficient based on trajectory type
        let kd = match self.trajectory {
            TrajectoryType::Liner => 0.75,
            TrajectoryType::Fly | TrajectoryType::PopUp => 0.55,
            TrajectoryType::Grounder => return 0.0, // Grounder height is always 0
        };

        // 2. Back-calculate time (t) to reach the target distance
        let horizontal_velocity = self.launch_speed * theta.cos() * kd;
        if horizontal_velocity <= 0.0 {
            return 0.0;
        } // Error guard

        let t = target_distance / horizontal_velocity;

        // 3. Calculate height at that time using the parabolic formula
        let initial_vertical_velocity = self.launch_speed * theta.sin();
        let height = (initial_vertical_velocity * t) - (0.5 * GRAVITY * t * t);

        // Clamp to 0m if negative (ball would be below ground)
        height.max(0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BallLocation {
    // x: -1.0 (inside/right-handed batter) ~ +1.0 (outside/right-handed batter)
    pub x: f64,
    // y: -1.0 (low) ~ +1.0 (high)
    pub y: f64,
}
impl BallLocation {
    /// Determine whether this is a ball (outside the strike zone)
    pub fn call(&self) -> PitchResult {
        if self.x.abs() > 1.0 || self.y.abs() > 1.0 {
            PitchResult::Ball
        } else {
            PitchResult::Strike
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BallMovement {
    pub x_m: f64,
    pub z_m: f64,
}

pub struct Zone {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}
impl Zone {
    pub fn width(&self) -> f64 {
        (self.x1 - self.x2).abs()
    }

    pub fn height(&self) -> f64 {
        (self.y1 - self.y2).abs()
    }
}

pub struct PitchedBall {
    pub pitch_type: PitchType,
    pub speed: f64,      // NOTE: (e.g., 41.67 m/s = 150.0 km/h)
    pub spin_rate: f64,  // NOTE: (e.g., 2300.0 rpm)
    pub spin_angle: f64, // NOTE: (e.g., 0.0 ~ 360.0 deg)
    pub spin_efficiency: f64,
    // NOTE: Spatial coordinates where the ball was released (m)
    // Example: x = -0.5 (right-handed pitcher's arm side), y = 16.5 (Extension 1.9m), z = 1.8 (release height)
    pub release_point: Vector3D,
    pub flight_time: f64,
    pub aim_zone: TargetZone,
    pub aim_location: BallLocation,
    pub actual_location: BallLocation,
}
impl PitchedBall {
    /// Returns lateral Magnus acceleration (m/s²)
    /// (+: acceleration to the right / -: acceleration to the left)
    pub fn get_side_accel(&self) -> f64 {
        // 1. Effective spin rate (rpm) contributing to Magnus force
        let effective_spin = self.spin_rate * self.spin_efficiency;

        // 2. Calculate total Magnus acceleration (unit: m/s²)
        let total_magnus_accel = MAGNUS_COEFF * effective_spin * self.speed;

        // 3. Extract lateral component (sin) from spin angle (deg)
        let dir_rad = self.spin_angle * PI / 180.0;
        let side_factor = dir_rad.sin();

        // 4. Lateral acceleration (m/s²)
        total_magnus_accel * side_factor
    }

    /// (Reference) Vertical Magnus acceleration (m/s²) follows the same logic
    pub fn get_vertical_accel(&self) -> f64 {
        // 1. Effective spin rate (rpm) contributing to Magnus force
        let effective_spin = self.spin_rate * self.spin_efficiency;

        // 2. Calculate total Magnus acceleration (unit: m/s²)
        let total_magnus_accel = MAGNUS_COEFF * effective_spin * self.speed;

        // 3. Extract lateral component (sin) from spin angle (deg)
        let dir_rad = self.spin_angle * PI / 180.0;
        let vertical_factor = dir_rad.cos(); // Vertical component uses cos (positive for backspin)

        // 4. Lateral acceleration (m/s²)
        total_magnus_accel * vertical_factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::util::GRAVITY;

    fn assert_near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {actual} to be near {expected}"
        );
    }

    fn ball(
        trajectory: TrajectoryType,
        distance: f64,
        spray_angle: f64,
        launch_speed_kmh: f64,
        launch_angle: f64,
    ) -> BattedBall {
        BattedBall::new(
            launch_speed_kmh,
            launch_angle,
            spray_angle,
            distance,
            2.5,
            trajectory,
        )
    }

    fn pitched_ball(
        speed: f64,
        spin_rate: f64,
        spin_angle: f64,
        spin_efficiency: f64,
    ) -> PitchedBall {
        PitchedBall {
            pitch_type: PitchType::FourSeamFastball,
            speed,
            spin_rate,
            spin_angle,
            spin_efficiency,
            release_point: Vector3D {
                x: 0.0,
                y: 16.0,
                z: 1.8,
            },
            flight_time: 0.4,
            aim_zone: TargetZone::Center,
            aim_location: BallLocation { x: 0.0, y: 0.0 },
            actual_location: BallLocation { x: 0.0, y: 0.0 },
        }
    }

    #[test]
    fn new_sets_physical_values_and_polar_position() {
        let ball = BattedBall::new(144.0, 30.0, 30.0, 100.0, 4.2, TrajectoryType::Fly);

        assert_near(ball.launch_speed, 144.0);
        assert_near(ball.launch_angle, 30.0);
        assert_near(ball.distance(), 100.0);
        assert_near(ball.angle(), 30.0);
        assert_near(ball.hang_time, 4.2);
        assert_eq!(ball.trajectory, TrajectoryType::Fly);
        assert_near(ball.x(), 50.0);
        assert_near(ball.y(), 100.0 * 30.0_f64.to_radians().cos());
    }

    #[test]
    fn azimuth_converts_launch_angle_to_radians() {
        let ball = ball(TrajectoryType::Fly, 90.0, 0.0, 100.0, 45.0);

        assert_near(ball.azimuth(), std::f64::consts::FRAC_PI_4);
    }

    #[test]
    fn is_foul_includes_angles_inside_foul_degree_boundary() {
        assert!(ball(TrajectoryType::Grounder, 30.0, -45.0, 80.0, 5.0).is_foul());
        assert!(!ball(TrajectoryType::Grounder, 30.0, 0.0, 80.0, 5.0).is_foul());
        assert!(ball(TrajectoryType::Grounder, 30.0, 45.0, 80.0, 5.0).is_foul());
    }

    #[test]
    fn is_foul_excludes_angles_outside_foul_degree_boundary() {
        assert!(ball(TrajectoryType::Fly, 80.0, -45.1, 100.0, 30.0).is_foul());
        assert!(ball(TrajectoryType::Fly, 80.0, 45.1, 100.0, 30.0).is_foul());
    }

    #[test]
    fn is_infield_uses_strict_distance_boundary() {
        assert!(ball(TrajectoryType::Grounder, 49.9, 0.0, 80.0, 5.0).is_infield());
        assert!(!ball(TrajectoryType::Grounder, 50.0, 0.0, 80.0, 5.0).is_infield());
    }

    #[test]
    fn is_shallow_uses_strict_distance_boundary() {
        assert!(ball(TrajectoryType::Liner, 44.9, 0.0, 90.0, 12.0).is_shallow());
        assert!(!ball(TrajectoryType::Liner, 45.0, 0.0, 90.0, 12.0).is_shallow());
    }

    #[test]
    fn calculate_height_at_distance_returns_zero_for_grounder() {
        let grounder = ball(TrajectoryType::Grounder, 50.0, 0.0, 100.0, 5.0);

        assert_near(grounder.calculate_height_at_distance(20.0), 0.0);
    }

    #[test]
    fn calculate_height_at_distance_uses_liner_drag_coefficient() {
        let liner = ball(TrajectoryType::Liner, 80.0, 0.0, 120.0, 20.0);
        let target_distance = 30.0;
        let v = 120.0 * 0.2778;
        let theta = 20.0_f64.to_radians();
        let t = target_distance / (v * theta.cos() * 0.75);
        let expected_height = (v * theta.sin() * t) - (0.5 * GRAVITY * t * t);

        assert_near(
            liner.calculate_height_at_distance(target_distance),
            expected_height,
        );
    }

    #[test]
    fn calculate_height_at_distance_uses_airborne_drag_coefficient() {
        let fly = ball(TrajectoryType::Fly, 100.0, 0.0, 140.0, 35.0);
        let popup = ball(TrajectoryType::PopUp, 35.0, 0.0, 140.0, 35.0);

        assert_near(
            fly.calculate_height_at_distance(25.0),
            popup.calculate_height_at_distance(25.0),
        );
    }

    #[test]
    fn calculate_height_at_distance_returns_zero_when_horizontal_velocity_is_invalid() {
        let backward_launch = ball(TrajectoryType::Fly, 80.0, 0.0, 100.0, 100.0);

        assert_near(backward_launch.calculate_height_at_distance(20.0), 0.0);
    }

    #[test]
    fn get_side_accel_extracts_lateral_spin_component() {
        let ball = pitched_ball(150.0, 2500.0, 90.0, 1.0);
        let expected_total_magnus_accel = MAGNUS_COEFF * 2500.0 * (150.0 / 3.6);

        assert_near(ball.get_side_accel(), expected_total_magnus_accel);
        assert_near(ball.get_vertical_accel(), 0.0);
    }

    #[test]
    fn get_side_accel_preserves_lateral_direction() {
        let ball = pitched_ball(150.0, 2500.0, 270.0, 1.0);
        let expected_total_magnus_accel = MAGNUS_COEFF * 2500.0 * (150.0 / 3.6);

        assert_near(ball.get_side_accel(), -expected_total_magnus_accel);
        assert_near(ball.get_vertical_accel(), 0.0);
    }

    #[test]
    fn get_vertical_accel_extracts_vertical_spin_component() {
        let backspin = pitched_ball(150.0, 2500.0, 0.0, 1.0);
        let topspin = pitched_ball(150.0, 2500.0, 180.0, 1.0);
        let expected_total_magnus_accel = MAGNUS_COEFF * 2500.0 * (150.0 / 3.6);

        assert_near(backspin.get_side_accel(), 0.0);
        assert_near(backspin.get_vertical_accel(), expected_total_magnus_accel);
        assert_near(topspin.get_side_accel(), 0.0);
        assert_near(topspin.get_vertical_accel(), -expected_total_magnus_accel);
    }

    #[test]
    fn pitched_ball_accel_scales_with_spin_efficiency() {
        let ball = pitched_ball(144.0, 2000.0, 45.0, 0.75);
        let expected_total_magnus_accel = MAGNUS_COEFF * (2000.0 * 0.75) * (144.0 / 3.6);
        let expected_component = expected_total_magnus_accel * 45.0_f64.to_radians().sin();

        assert_near(ball.get_side_accel(), expected_component);
        assert_near(ball.get_vertical_accel(), expected_component);
    }
}
