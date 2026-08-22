use crate::domain::shared::game::PitchResult;
use crate::domain::shared::player::RL;
use crate::domain::shared::player::{PitchType, Position};
use crate::domain::strategy::pitching_strategy::TargetZone;
use crate::domain::util::{PolarPosition, Vector3D};
use crate::t;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use std::fmt;
use strum::IntoEnumIterator;
use strum_macros::{AsRefStr, EnumString};

pub const FOUL_DEGREE: f64 = 45.0;
const INFIELD_DISTANCE: f64 = 50.0;
const SHALLOW_DISTANCE: f64 = 45.0;

// Scaling coefficient calibrated so that at 2500rpm, 150km/h (41.67m/s), efficiency 1.0, acceleration is approx. 3.5 m/s²
// Coefficient K ≈ 3.5 / (2500.0 * 41.67) ≈ 0.0000336
pub const MAGNUS_COEFF: f64 = 0.0000336;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, AsRefStr, Serialize, Deserialize)]
pub enum OutboundResult {
    InField,
    HomeRun,
    GroundRuleDouble,
    Foul,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, AsRefStr, Serialize, Deserialize)]
pub enum TrajectoryType {
    Grounder,
    Liner,
    Fly,
    PopUp,
    NA,
}
impl fmt::Display for TrajectoryType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TrajectoryType::Grounder => write!(f, "{}", t!("grounder")),
            TrajectoryType::Liner => write!(f, "{}", t!("liner")),
            TrajectoryType::Fly => write!(f, "{}", t!("fly")),
            TrajectoryType::PopUp => write!(f, "{}", t!("popup")),
            TrajectoryType::NA => write!(f, "{}", t!("na")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FieldedBall {
    pub ball: BattedBall,
    pub fielded_by: Position,
    pub catch_position: PolarPosition,
    pub time_to_field: f64,
    pub is_fly_catch: bool,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct BattedBall {
    pub launch_speed: f64,
    pub launch_angle: f64,
    pub spin_rate: f64,
    pub spin_angle: f64,
    pub final_position: PolarPosition,
    pub max_height: f64,
    pub total_time: f64,
    // NOTE: Polar coordinates (r) and time (t) of the first bounce
    // Note: None if it hits the fence directly or is a no-bounce home run
    pub first_bounce_position: Option<PolarPosition>,
    pub first_bounce_time: Option<f64>,
    pub fence_impact_position: Option<PolarPosition>,
    pub fence_impact_time: Option<f64>,
    pub outbound_result: OutboundResult,
}

impl BattedBall {
    pub fn new(
        launch_speed: f64,
        launch_angle: f64,
        polar_distance: f64,
        polar_angle: f64,
        total_time: f64,
        first_bounce_position: Option<PolarPosition>,
        first_bounce_time: Option<f64>,
        fence_impact_position: Option<PolarPosition>,
        fence_impact_time: Option<f64>,
        outbound_result: OutboundResult,
    ) -> Self {
        Self {
            launch_speed,
            launch_angle,
            spin_rate: 0.0,
            spin_angle: 0.0,
            final_position: PolarPosition::new(polar_distance, polar_angle),
            max_height: 0.0,
            total_time,
            first_bounce_position,
            first_bounce_time,
            fence_impact_position,
            fence_impact_time,
            outbound_result,
        }
    }

    pub fn default() -> Self {
        Self {
            launch_speed: 0.0,
            launch_angle: 0.0,
            spin_rate: 0.0,
            spin_angle: 0.0,
            final_position: PolarPosition::new(0.0, 0.0),
            max_height: 0.0,
            total_time: 0.0,
            first_bounce_position: None,
            first_bounce_time: None,
            fence_impact_position: None,
            fence_impact_time: None,
            outbound_result: OutboundResult::InField,
        }
    }

    pub fn distance(&self) -> f64 {
        self.final_position.distance
    }

    pub fn angle(&self) -> f64 {
        self.final_position.angle
    }

    pub fn azimuth(&self) -> f64 {
        self.launch_angle.to_radians()
    }

    pub fn x(&self) -> f64 {
        self.final_position.x
    }

    pub fn y(&self) -> f64 {
        self.final_position.y
    }

    pub fn is_foul(&self) -> bool {
        if self.final_position.angle.abs() >= FOUL_DEGREE {
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

    /// Determine final batted ball category by combining launch angle and spin
    pub fn trajectory(&self) -> TrajectoryType {
        // 1. Calculate trajectory correction from spin (lift/sink)
        // Backspin (0°) gives positive correction, topspin (180°) gives negative correction
        let spin_angle_rad = self.spin_angle.to_radians();
        let backspin_factor = spin_angle_rad.cos(); // 0 deg => 1.0, 180 deg => -1.0

        // Lift/sink correction proportional to spin rate (approximately ±a few degrees)
        let spin_lift_effect = (self.spin_rate / 2000.0) * backspin_factor * 3.5;

        // Effective angle incorporating spin effect
        let effective_angle = self.launch_angle + spin_lift_effect;

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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BallLocation {
    // x: -1.0 (inside/right-handed batter) ~ +1.0 (outside/right-handed batter)
    pub x: f64,
    // y: -1.0 (low) ~ +1.0 (high)
    pub y: f64,
}
impl BallLocation {
    // Determine whether this is a ball (outside the strike zone)
    pub fn call(&self, batting_side: RL) -> PitchResult {
        // 1. Wild pitch (area well beyond the catcher's catchable frame)
        if self.x.abs() > 3.2 || self.y > 3.5 || self.y < -2.2 {
            return PitchResult::WildPitch;
        }

        // 2. Hit by pitch (entering the batter's standing position / body area)
        // For right-handed batters: norm_x is largely negative (inside) and at body height (y)
        let body_side_x = if batting_side == RL::Right {
            -self.x
        } else {
            self.x
        };
        if body_side_x > 1.8 && self.y.abs() < 2.5 {
            return PitchResult::HitByPitch;
        }

        // 3. Normal strike / ball call
        if self.x.abs() > 1.0 || self.y.abs() > 1.0 {
            PitchResult::Ball
        } else {
            PitchResult::Strike
        }
    }

    pub fn target_zone(&self) -> TargetZone {
        TargetZone::iter()
            .find(|target_zone| target_zone.zone().is_in_zone(*self))
            .unwrap_or_else(|| panic!("ball location is outside all target zones: {self:?}"))
    }

    // Physical ball distance from the strike zone center (degree of deviation)
    pub fn distance_from_zone_edge(&self) -> f64 {
        let x_out = (self.x.abs() - 1.0).max(0.0);
        let y_out = (self.y.abs() - 1.0).max(0.0);
        (x_out.powi(2) + y_out.powi(2)).sqrt()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BallMovement {
    pub x_m: f64,
    pub z_m: f64,
}

pub struct BallZone {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}
impl BallZone {
    pub fn width(&self) -> f64 {
        (self.x1 - self.x2).abs()
    }

    pub fn height(&self) -> f64 {
        (self.y1 - self.y2).abs()
    }

    pub fn is_in_zone(&self, location: BallLocation) -> bool {
        let min_x = self.x1.min(self.x2);
        let max_x = self.x1.max(self.x2);
        let min_y = self.y1.min(self.y2);
        let max_y = self.y1.max(self.y2);

        location.x >= min_x && location.x <= max_x && location.y >= min_y && location.y <= max_y
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
    // Returns lateral Magnus acceleration (m/s²)
    // (+: acceleration to the right / -: acceleration to the left)
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

    // (Reference) Vertical Magnus acceleration (m/s²) follows the same logic
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

    fn assert_near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {actual} to be near {expected}"
        );
    }

    fn ball(distance: f64, spray_angle: f64, launch_speed: f64, launch_angle: f64) -> BattedBall {
        BattedBall {
            launch_speed,
            launch_angle,
            spin_rate: 0.0,
            spin_angle: 0.0,
            final_position: PolarPosition::new(distance, spray_angle),
            max_height: 0.0,
            total_time: 2.5,
            first_bounce_position: None,
            first_bounce_time: None,
            fence_impact_position: None,
            fence_impact_time: None,
            outbound_result: OutboundResult::InField,
        }
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
    fn ball_zone_includes_locations_inside_and_on_boundary() {
        let zone = BallZone {
            x1: -1.0,
            y1: 1.0,
            x2: 0.0,
            y2: 0.0,
        };

        assert!(zone.is_in_zone(BallLocation { x: -0.5, y: 0.5 }));
        assert!(zone.is_in_zone(BallLocation { x: -1.0, y: 1.0 }));
        assert!(zone.is_in_zone(BallLocation { x: 0.0, y: 0.0 }));
    }

    #[test]
    fn ball_zone_excludes_locations_outside() {
        let zone = BallZone {
            x1: -1.0,
            y1: 1.0,
            x2: 0.0,
            y2: 0.0,
        };

        assert!(!zone.is_in_zone(BallLocation { x: -1.1, y: 0.5 }));
        assert!(!zone.is_in_zone(BallLocation { x: -0.5, y: 1.1 }));
        assert!(!zone.is_in_zone(BallLocation { x: 0.1, y: 0.5 }));
        assert!(!zone.is_in_zone(BallLocation { x: -0.5, y: -0.1 }));
    }

    #[test]
    fn ball_zone_accepts_either_corner_order() {
        let zone = BallZone {
            x1: 0.0,
            y1: 0.0,
            x2: -1.0,
            y2: 1.0,
        };

        assert!(zone.is_in_zone(BallLocation { x: -0.5, y: 0.5 }));
        assert!(!zone.is_in_zone(BallLocation { x: 0.1, y: 0.5 }));
    }

    #[test]
    fn target_zone_returns_matching_zone() {
        assert_eq!(
            BallLocation { x: -0.75, y: -0.75 }.target_zone(),
            TargetZone::LowInside
        );
        assert_eq!(
            BallLocation { x: 0.75, y: -0.75 }.target_zone(),
            TargetZone::LowOutside
        );
        assert_eq!(
            BallLocation { x: -0.75, y: 0.75 }.target_zone(),
            TargetZone::HighInside
        );
        assert_eq!(
            BallLocation { x: 0.75, y: 0.75 }.target_zone(),
            TargetZone::HighOutside
        );
    }

    #[test]
    fn target_zone_prefers_center_when_zones_overlap() {
        assert_eq!(
            BallLocation { x: 0.0, y: 0.0 }.target_zone(),
            TargetZone::Center
        );
    }

    #[test]
    fn default_sets_zeroed_physical_values_and_empty_event_metadata() {
        let ball = BattedBall::default();

        assert_near(ball.launch_speed, 0.0);
        assert_near(ball.launch_angle, 0.0);
        assert_near(ball.spin_rate, 0.0);
        assert_near(ball.spin_angle, 0.0);
        assert_near(ball.distance(), 0.0);
        assert_near(ball.angle(), 0.0);
        assert_near(ball.total_time, 0.0);
        assert_eq!(ball.first_bounce_position, None);
        assert_eq!(ball.first_bounce_time, None);
        assert_eq!(ball.outbound_result, OutboundResult::InField);
    }

    #[test]
    fn accessors_read_final_polar_position() {
        let ball = ball(100.0, 30.0, 144.0, 30.0);

        assert_near(ball.distance(), 100.0);
        assert_near(ball.angle(), 30.0);
        assert_near(ball.x(), 50.0);
        assert_near(ball.y(), 100.0 * 30.0_f64.to_radians().cos());
    }

    #[test]
    fn azimuth_converts_launch_angle_to_radians() {
        let ball = ball(90.0, 0.0, 100.0, 45.0);

        assert_near(ball.azimuth(), std::f64::consts::FRAC_PI_4);
    }

    #[test]
    fn is_foul_includes_angles_inside_foul_degree_boundary() {
        assert!(ball(30.0, -45.0, 80.0, 5.0).is_foul());
        assert!(!ball(30.0, 0.0, 80.0, 5.0).is_foul());
        assert!(ball(30.0, 45.0, 80.0, 5.0).is_foul());
    }

    #[test]
    fn is_foul_excludes_angles_outside_foul_degree_boundary() {
        assert!(ball(80.0, -45.1, 100.0, 30.0).is_foul());
        assert!(ball(80.0, 45.1, 100.0, 30.0).is_foul());
    }

    #[test]
    fn is_infield_uses_strict_distance_boundary() {
        assert!(ball(49.9, 0.0, 80.0, 5.0).is_infield());
        assert!(!ball(50.0, 0.0, 80.0, 5.0).is_infield());
    }

    #[test]
    fn is_shallow_uses_strict_distance_boundary() {
        assert!(ball(44.9, 0.0, 90.0, 12.0).is_shallow());
        assert!(!ball(45.0, 0.0, 90.0, 12.0).is_shallow());
    }

    #[test]
    fn trajectory_classifies_by_launch_angle_without_spin() {
        assert_eq!(
            ball(30.0, 0.0, 80.0, 9.9).trajectory(),
            TrajectoryType::Grounder
        );
        assert_eq!(
            ball(80.0, 0.0, 100.0, 10.0).trajectory(),
            TrajectoryType::Liner
        );
        assert_eq!(
            ball(100.0, 0.0, 120.0, 25.0).trajectory(),
            TrajectoryType::Fly
        );
        assert_eq!(
            ball(70.0, 0.0, 90.0, 50.0).trajectory(),
            TrajectoryType::PopUp
        );
    }

    #[test]
    fn trajectory_applies_backspin_and_topspin_lift_correction() {
        let backspin_liner = BattedBall {
            spin_rate: 2000.0,
            spin_angle: 0.0,
            ..ball(80.0, 0.0, 100.0, 7.0)
        };
        let topspin_grounder = BattedBall {
            spin_rate: 2000.0,
            spin_angle: 180.0,
            ..ball(80.0, 0.0, 100.0, 12.0)
        };

        assert_eq!(backspin_liner.trajectory(), TrajectoryType::Liner);
        assert_eq!(topspin_grounder.trajectory(), TrajectoryType::Grounder);
    }

    #[test]
    fn get_side_accel_extracts_lateral_spin_component() {
        let speed = 150.0 / 3.6;
        let ball = pitched_ball(speed, 2500.0, 90.0, 1.0);
        let expected_total_magnus_accel = MAGNUS_COEFF * 2500.0 * speed;

        assert_near(ball.get_side_accel(), expected_total_magnus_accel);
        assert_near(ball.get_vertical_accel(), 0.0);
    }

    #[test]
    fn get_side_accel_preserves_lateral_direction() {
        let speed = 150.0 / 3.6;
        let ball = pitched_ball(speed, 2500.0, 270.0, 1.0);
        let expected_total_magnus_accel = MAGNUS_COEFF * 2500.0 * speed;

        assert_near(ball.get_side_accel(), -expected_total_magnus_accel);
        assert_near(ball.get_vertical_accel(), 0.0);
    }

    #[test]
    fn get_vertical_accel_extracts_vertical_spin_component() {
        let speed = 150.0 / 3.6;
        let backspin = pitched_ball(speed, 2500.0, 0.0, 1.0);
        let topspin = pitched_ball(speed, 2500.0, 180.0, 1.0);
        let expected_total_magnus_accel = MAGNUS_COEFF * 2500.0 * speed;

        assert_near(backspin.get_side_accel(), 0.0);
        assert_near(backspin.get_vertical_accel(), expected_total_magnus_accel);
        assert_near(topspin.get_side_accel(), 0.0);
        assert_near(topspin.get_vertical_accel(), -expected_total_magnus_accel);
    }

    #[test]
    fn pitched_ball_accel_scales_with_spin_efficiency() {
        let speed = 144.0 / 3.6;
        let ball = pitched_ball(speed, 2000.0, 45.0, 0.75);
        let expected_total_magnus_accel = MAGNUS_COEFF * (2000.0 * 0.75) * speed;
        let expected_component = expected_total_magnus_accel * 45.0_f64.to_radians().sin();

        assert_near(ball.get_side_accel(), expected_component);
        assert_near(ball.get_vertical_accel(), expected_component);
    }
}
