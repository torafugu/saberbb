use crate::t;
use std::fmt;

const FOUL_DEGREE: f64 = 45.0;

#[derive(Debug, Clone)]
pub struct PolarPosition {
    pub distance: f64, // Distance from home plate in meters
    pub angle: f64, // Angle in degrees. 0° points toward second base, positive values go clockwise
    pub x: f64,
    pub y: f64,
}
impl PolarPosition {
    pub fn new(distance: f64, angle: f64) -> Self {
        let angle_rad = angle.to_radians();
        let x = distance * angle_rad.sin();
        let y = distance * angle_rad.cos();

        Self {
            distance: distance,
            angle: angle,
            x: x,
            y: y,
        }
    }
}

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

pub struct Ball {
    pub launch_speed: f64, // km/h
    pub launch_angle: f64, // Z arc degree
    pub polar_position: PolarPosition,
    pub hang_time: f64, // second
    pub trajectory: TrajectoryType,
}

impl Ball {
    pub fn new(
        launch_speed: f64,
        launch_angle: f64,
        spray_angle: f64,
        distance: f64,
        hang_time: f64,
        trajectory: TrajectoryType,
    ) -> Self {
        Self {
            launch_speed: launch_speed,
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

    pub fn is_foul(&self) -> bool {
        if self.polar_position.angle.abs() <= FOUL_DEGREE {
            true
        } else {
            false
        }
    }
}
