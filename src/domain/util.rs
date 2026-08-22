use crate::domain::shared::game::BaseCode;
use crate::error::AppError;
use serde::{Deserialize, Serialize};

pub const GRAVITY: f64 = 9.81;

pub struct Vector3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Data structure representing a single peak (or trough) within the ball zone
#[derive(Debug, Clone)]
pub struct GaussianPeak {
    pub center_x: f64,  // Center coordinate X (e.g. strike zone width -1.0 ~ 1.0)
    pub center_y: f64,  // Center coordinate Y (e.g. strike zone height -1.0 ~ 1.0)
    pub amplitude: f64, // Amplitude A_i (positive = strength, negative = weakness)
    pub sigma_x: f64,   // Spread in the X direction σ_x
    pub sigma_y: f64,   // Spread in the Y direction σ_y
}

impl GaussianPeak {
    /// Calculate this peak's contribution at the specified coordinates (x, y)
    #[inline]
    pub fn evaluate(&self, x: f64, y: f64) -> f64 {
        let dx2 = (x - self.center_x).powi(2) / (2.0 * self.sigma_x.powi(2));
        let dy2 = (y - self.center_y).powi(2) / (2.0 * self.sigma_y.powi(2));
        self.amplitude * (-dx2 - dy2).exp()
    }
}

pub fn euclidean_distance(a: &[f64], b: &[f64]) -> Result<f64, AppError> {
    if a.len() != b.len() {
        return Err(AppError::InvalidInput(
            "Mismatched vector dimensions".to_string(),
        ));
    }

    let sum_sq: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let diff = x - y;
            diff * diff
        })
        .sum();

    Ok(sum_sq.sqrt())
}

pub fn softmax(values: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return Vec::new();
    }

    let max_value = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    let exp_values: Vec<f64> = values.iter().map(|&x| (x - max_value).exp()).collect();

    let sum: f64 = exp_values.iter().sum();

    exp_values.into_iter().map(|x| x / sum).collect()
}

pub fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

pub fn is_base_occupied(bases_occupied: u8, base: BaseCode) -> bool {
    (bases_occupied & (1 << base as u8)) != 0
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub struct PolarPosition {
    pub distance: f64, // NOTE: Distance from home plate in meters
    pub angle: f64, // NOTE: Angle in degrees. 0° points toward second base, positive values go clockwise
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

pub fn calculate_polar_distance(p1: &PolarPosition, p2: &PolarPosition) -> f64 {
    // Convert the difference between the two angles to radians.
    let angle_diff_rad = (p1.angle - p2.angle).to_radians();

    // Apply the law of cosines.
    let cos_val = angle_diff_rad.cos();
    let distance_squared = (p1.distance * p1.distance) + (p2.distance * p2.distance)
        - (2.0 * p1.distance * p2.distance * cos_val);

    // Guard against rare negative values caused by floating-point error.
    distance_squared.max(0.0).sqrt()
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

    // ── sigmoid ──────────────────────────────────────────────

    #[test]
    fn test_sigmoid_center() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_sigmoid_large_positive() {
        assert!((sigmoid(10.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_sigmoid_large_negative() {
        assert!((sigmoid(-10.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_sigmoid_symmetry() {
        let x = 2.5;
        assert!((sigmoid(x) - (1.0 - sigmoid(-x))).abs() < 1e-9);
    }

    #[test]
    fn test_sigmoid_monotonic() {
        let a = sigmoid(-1.0);
        let b = sigmoid(0.0);
        let c = sigmoid(1.0);
        assert!(a < b && b < c);
    }

    #[test]
    fn test_sigmoid_extreme_values() {
        assert!(sigmoid(100.0).is_finite());
        assert!(sigmoid(-100.0).is_finite());
    }

    // ── is_base_occupied ─────────────────────────────────────

    #[test]
    fn test_is_base_occupied_first() {
        assert!(is_base_occupied(0b001, BaseCode::First));
        assert!(!is_base_occupied(0b001, BaseCode::Second));
        assert!(!is_base_occupied(0b001, BaseCode::Third));
    }

    #[test]
    fn test_is_base_occupied_second() {
        assert!(!is_base_occupied(0b010, BaseCode::First));
        assert!(is_base_occupied(0b010, BaseCode::Second));
        assert!(!is_base_occupied(0b010, BaseCode::Third));
    }

    #[test]
    fn test_is_base_occupied_third() {
        assert!(!is_base_occupied(0b100, BaseCode::First));
        assert!(!is_base_occupied(0b100, BaseCode::Second));
        assert!(is_base_occupied(0b100, BaseCode::Third));
    }

    #[test]
    fn test_is_base_occupied_empty() {
        assert!(!is_base_occupied(0b000, BaseCode::First));
        assert!(!is_base_occupied(0b000, BaseCode::Second));
        assert!(!is_base_occupied(0b000, BaseCode::Third));
    }

    #[test]
    fn test_is_base_occupied_bases_loaded() {
        assert!(is_base_occupied(0b111, BaseCode::First));
        assert!(is_base_occupied(0b111, BaseCode::Second));
        assert!(is_base_occupied(0b111, BaseCode::Third));
    }

    #[test]
    fn test_is_base_occupied_ignores_higher_bits() {
        // Upper bits beyond bit 2 should be ignored
        assert!(is_base_occupied(0b101101, BaseCode::First));
        assert!(!is_base_occupied(0b101101, BaseCode::Second));
        assert!(is_base_occupied(0b101101, BaseCode::Third));
    }

    // ── PolarPosition ────────────────────────────────────────

    #[test]
    fn test_polar_position_zero_angle_points_to_second_base() {
        let position = PolarPosition::new(42.0, 0.0);

        assert_eq!(position.distance, 42.0);
        assert_eq!(position.angle, 0.0);
        assert_near(position.x, 0.0);
        assert_near(position.y, 42.0);
    }

    #[test]
    fn test_polar_position_positive_angle_goes_clockwise() {
        let position = PolarPosition::new(10.0, 90.0);

        assert_near(position.x, 10.0);
        assert_near(position.y, 0.0);
    }

    #[test]
    fn test_polar_position_negative_angle_goes_counterclockwise() {
        let position = PolarPosition::new(10.0, -90.0);

        assert_near(position.x, -10.0);
        assert_near(position.y, 0.0);
    }

    #[test]
    fn test_polar_position_forty_five_degrees_uses_equal_coordinates() {
        let position = PolarPosition::new(10.0 * 2.0_f64.sqrt(), 45.0);

        assert_near(position.x, 10.0);
        assert_near(position.y, 10.0);
    }

    #[test]
    fn test_polar_position_zero_distance_stays_at_home_plate() {
        let position = PolarPosition::new(0.0, 33.0);

        assert_near(position.x, 0.0);
        assert_near(position.y, 0.0);
    }

    // ── calculate_distance ───────────────────────────────────

    #[test]
    fn test_calculate_distance_same_position_is_zero() {
        let position = PolarPosition::new(50.0, 12.0);

        assert_near(calculate_polar_distance(&position, &position), 0.0);
    }

    #[test]
    fn test_calculate_distance_same_angle_uses_distance_difference() {
        let p1 = PolarPosition::new(40.0, 15.0);
        let p2 = PolarPosition::new(25.0, 15.0);

        assert_near(calculate_polar_distance(&p1, &p2), 15.0);
    }

    #[test]
    fn test_calculate_distance_right_angle_uses_pythagorean_distance() {
        let p1 = PolarPosition::new(3.0, 0.0);
        let p2 = PolarPosition::new(4.0, 90.0);

        assert_near(calculate_polar_distance(&p1, &p2), 5.0);
    }

    #[test]
    fn test_calculate_distance_opposite_angles_adds_distances() {
        let p1 = PolarPosition::new(12.0, 0.0);
        let p2 = PolarPosition::new(8.0, 180.0);

        assert_near(calculate_polar_distance(&p1, &p2), 20.0);
    }

    #[test]
    fn test_calculate_distance_is_symmetric() {
        let p1 = PolarPosition::new(90.0, -26.0);
        let p2 = PolarPosition::new(80.0, 31.0);

        assert_near(
            calculate_polar_distance(&p1, &p2),
            calculate_polar_distance(&p2, &p1),
        );
    }
}
