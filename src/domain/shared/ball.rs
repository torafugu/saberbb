use crate::domain::util::{GRAVIY, PolarPosition};
use crate::t;
use std::fmt;
use strum_macros::{AsRefStr, EnumString};

const FOUL_DEGREE: f64 = 45.0;
const INFIELD_DISTANCE: f64 = 50.0;
const SHALLOW_DISTANCE: f64 = 45.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, AsRefStr)]
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
pub struct Ball {
    pub launch_speed_kmh: f64,
    pub launch_angle: f64, // Z arc degree
    pub polar_position: PolarPosition,
    pub hang_time: f64, // second
    pub trajectory: TrajectoryType,
}

impl Ball {
    pub fn new(
        launch_speed_kmh: f64,
        launch_angle: f64,
        spray_angle: f64,
        distance: f64,
        hang_time: f64,
        trajectory: TrajectoryType,
    ) -> Self {
        Self {
            launch_speed_kmh,
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

    pub fn launch_speed_ms(&self) -> f64 {
        self.launch_speed_kmh * 0.278
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
        let v = self.launch_speed_kmh * 0.278; // Convert to m/s
        let theta = self.launch_angle.to_radians();

        // 1. Apply drag coefficient based on trajectory type
        let kd = match self.trajectory {
            TrajectoryType::Liner => 0.75,
            TrajectoryType::Fly | TrajectoryType::PopUp => 0.55,
            TrajectoryType::Grounder => return 0.0, // Grounder height is always 0
        };

        // 2. Back-calculate time (t) to reach the target distance
        let horizontal_velocity = v * theta.cos() * kd;
        if horizontal_velocity <= 0.0 {
            return 0.0;
        } // Error guard

        let t = target_distance / horizontal_velocity;

        // 3. Calculate height at that time using the parabolic formula
        let initial_vertical_velocity = v * theta.sin();
        let height = (initial_vertical_velocity * t) - (0.5 * GRAVIY * t * t);

        // Clamp to 0m if negative (ball would be below ground)
        height.max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::util::GRAVIY;

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
    ) -> Ball {
        Ball::new(
            launch_speed_kmh,
            launch_angle,
            spray_angle,
            distance,
            2.5,
            trajectory,
        )
    }

    #[test]
    fn new_sets_physical_values_and_polar_position() {
        let ball = Ball::new(144.0, 30.0, 30.0, 100.0, 4.2, TrajectoryType::Fly);

        assert_near(ball.launch_speed_kmh, 144.0);
        assert_near(ball.launch_angle, 30.0);
        assert_near(ball.distance(), 100.0);
        assert_near(ball.angle(), 30.0);
        assert_near(ball.hang_time, 4.2);
        assert_eq!(ball.trajectory, TrajectoryType::Fly);
        assert_near(ball.x(), 50.0);
        assert_near(ball.y(), 100.0 * 30.0_f64.to_radians().cos());
    }

    #[test]
    fn launch_speed_ms_converts_kmh_to_meters_per_second() {
        let ball = ball(TrajectoryType::Liner, 90.0, 0.0, 150.0, 20.0);

        assert_near(ball.launch_speed_ms(), 41.7);
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
        let v = 120.0 * 0.278;
        let theta = 20.0_f64.to_radians();
        let t = target_distance / (v * theta.cos() * 0.75);
        let expected_height = (v * theta.sin() * t) - (0.5 * GRAVIY * t * t);

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
}
