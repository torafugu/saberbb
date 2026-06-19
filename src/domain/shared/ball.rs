use crate::domain::util::{GRAVIY, PolarPosition};
use crate::t;
use std::fmt;

const FOUL_DEGREE: f64 = 45.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug)]
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
        if self.polar_position.angle.abs() <= FOUL_DEGREE {
            true
        } else {
            false
        }
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
