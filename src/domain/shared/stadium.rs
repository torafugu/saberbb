use crate::domain::shared::ball::{BattedBall, TrajectoryType};
use crate::domain::shared::game::BASE_DISTANCE;
use crate::domain::util::PolarPosition;
use crate::proj_dirs;
use crate::t;
use kurbo::{Affine, BezPath, CubicBez, Line, PathEl, Point, Shape, Vec2};
use serde::{Deserialize, Serialize};
use std::f64::consts::SQRT_2;
use strum_macros::{AsRefStr, EnumString};
use svg::Document;
use svg::node::element::path::Data;
use svg::node::element::{Circle, Line as svgLine, Path, Rectangle, Text};
use validator::Validate;

pub const MOUND_DISTANCE: f64 = 18.44;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, EnumString, AsRefStr)]
pub enum Base {
    Home,
    First,
    Second,
    Third,
}
impl Base {
    pub fn polar_position(&self) -> PolarPosition {
        let second_base_distance = BASE_DISTANCE * SQRT_2;
        match self.clone() {
            Base::Home => PolarPosition::new(0.0, 0.0),
            Base::First => PolarPosition::new(BASE_DISTANCE, 45.0),
            Base::Second => PolarPosition::new(second_base_distance, 0.0),
            Base::Third => PolarPosition::new(BASE_DISTANCE, -45.0),
        }
    }
}
impl std::fmt::Display for Base {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Base::Home => write!(f, "{}", t!("home")),
            Base::First => write!(f, "{}", t!("first")),
            Base::Second => write!(f, "{}", t!("second")),
            Base::Third => write!(f, "{}", t!("third")),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct Stadium {
    pub id: u16,
    pub name: String,
    pub foul_pole_distance: f64,
    pub center_fence_distance: f64,
    // pub fair_zone: kurbo::BezPath,
    pub(crate) fence_line: kurbo::BezPath,
    pub(crate) fence_height: f64,
}
impl Stadium {
    pub fn new(
        id: u16,
        name: String,
        foul_pole_distance: f64,
        center_fence_distance: f64,
        fence_height: f64,
    ) -> Self {
        let fence_line = Self::build_fence_line(foul_pole_distance, center_fence_distance);

        Self {
            id,
            name,
            foul_pole_distance,
            center_fence_distance,
            fence_line,
            fence_height,
        }
    }

    pub fn fence_line_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.fence_line)
    }

    pub fn build_fence_line_json(
        foul_pole_distance: f64,
        center_fence_distance: f64,
    ) -> Result<String, serde_json::Error> {
        serde_json::to_string(&Self::build_fence_line(
            foul_pole_distance,
            center_fence_distance,
        ))
    }

    fn build_fence_line(foul_pole_distance: f64, center_fence_distance: f64) -> BezPath {
        let homerun_pole_x = foul_pole_distance / SQRT_2;
        let homerun_pole_y = foul_pole_distance / SQRT_2;
        let center_fence_x = 0.0;
        let center_fence_y = center_fence_distance;
        let foul_fence_y = homerun_pole_y - foul_pole_distance * 0.065;

        let backnet_x = 0.14 * center_fence_distance;
        let backnet_y = -0.09 * center_fence_distance;
        let infield_x = 0.41 * center_fence_distance;
        let infield_y = 0.32 * center_fence_distance;
        let outfield_x1 = 0.534 * center_fence_distance;
        let outfield_y1 = 0.86 * center_fence_distance;
        let outfield_x2 = 0.3 * center_fence_distance;
        let outfield_y2 = 1.01 * center_fence_distance;

        let mut fence_line = BezPath::new();
        fence_line.move_to(Point::new(0.0, -10.0));
        fence_line.curve_to(
            Point::new(backnet_x, backnet_y),
            Point::new(infield_x, infield_y),
            Point::new(homerun_pole_x, foul_fence_y),
        );
        fence_line.curve_to(
            Point::new(outfield_x1, outfield_y1),
            Point::new(outfield_x2, outfield_y2),
            Point::new(center_fence_x, center_fence_y),
        );
        fence_line.curve_to(
            Point::new(-outfield_x2, outfield_y2),
            Point::new(-outfield_x1, outfield_y1),
            Point::new(-homerun_pole_x, foul_fence_y),
        );
        fence_line.curve_to(
            Point::new(-infield_x, infield_y),
            Point::new(-backnet_x, backnet_y),
            Point::new(0.0, -10.0),
        );
        fence_line.close_path();

        fence_line
    }

    pub fn draw_fence(&self) {
        let scale = 4.0; // 1m = 4px
        let svg_width = 800.0;
        let svg_height = 800.0;

        let home_x = svg_width / 2.0; // 400.0
        let home_y = svg_height - 50.0; // 750.0

        let to_svg =
            Affine::translate(Vec2::new(home_x, home_y)) * Affine::scale_non_uniform(scale, -scale);
        let fence_line_svg_path = to_svg * &self.fence_line;

        let fence_path = Path::new()
            .set("stroke", "white") // White line
            .set("stroke-width", 2)
            .set("fill", "#2e8b57")
            .set("d", fence_line_svg_path.to_svg());

        let fence_svg = Document::new()
            .set("viewBox", (0, 0, svg_width, svg_height))
            .set("width", svg_width)
            .set("height", svg_height)
            .add(fence_path);

        draw(fence_svg);
    }

    pub fn is_stand_in(&self, ball: &BattedBall) -> bool {
        if ball.trajectory == TrajectoryType::Grounder {
            return false;
        };

        if let Some(distance) = self.fence_distance_at_angle(ball.angle()) {
            if ball.distance() < distance {
                return false;
            }

            let ball_height = ball.calculate_height_at_distance(distance);
            if ball_height > self.fence_height {
                return true;
            } else {
                return false;
            }
        } else {
            return false;
        }
    }

    pub fn fence_distance_at_angle(&self, angle: f64) -> Option<f64> {
        self.fence_intersection_at_angle(angle)
            .map(|intersect_pt| intersect_pt.distance(Point::ORIGIN))
    }

    fn fence_intersection_at_angle(&self, angle: f64) -> Option<Point> {
        let ray_distance = self.center_fence_distance.max(self.foul_pole_distance) * 2.0;
        let ray_end_position = PolarPosition::new(ray_distance, angle);
        let ray = Line::new(
            Point::ORIGIN,
            Point {
                x: ray_end_position.x,
                y: ray_end_position.y,
            },
        );

        find_intersections(&self.fence_line, ray)
    }
}

impl Default for Stadium {
    fn default() -> Self {
        Self::new(1, "Stadium A".to_string(), 98.0, 120.0, 2.0)
    }
}

pub fn draw(document: Document) {
    let svg_path = proj_dirs().data_dir().join("stadium.svg");
    svg::save(svg_path, &document).unwrap();
}

pub fn generate_svg() -> Document {
    let scale: f64 = 5.0;
    let width = 900.0;
    let height = 820.0;

    let mut document = Document::new()
        .set("width", width)
        .set("height", height)
        .set("viewBox", format!("0 0 {} {}", width, height))
        .set("style", "background: #333");

    let home_x = width / 2.0;
    let home_y = height - 140.0;

    let px = |value: f64| value * scale;

    let base_dist = BASE_DISTANCE * scale;
    let offset = base_dist * 0.70710678118;

    let p1b_x = home_x + offset;
    let p1b_y = home_y - offset;
    let p2b_x = home_x;
    let p2b_y = home_y - base_dist * SQRT_2;
    let p3b_x = home_x - offset;
    let p3b_y = home_y - offset;

    // 1. Stands (outermost area)
    document = document.add(
        Rectangle::new()
            .set("x", 0)
            .set("y", 0)
            .set("width", width)
            .set("height", height)
            .set("fill", "#555555"),
    );

    // 2. Entire field following the stadium shape (foul territory)
    let foul_pole_distance = 97.0 * scale;
    let left_pole_x = home_x - foul_pole_distance / SQRT_2;
    let left_pole_y = home_y - foul_pole_distance / SQRT_2;
    let right_pole_x = home_x + foul_pole_distance / SQRT_2;
    let right_pole_y = left_pole_y;
    let center_fence_x = home_x;
    let center_fence_y = home_y - 120.0 * scale;
    let left_fence_y = left_pole_y + px(6.25);
    let right_fence_y = right_pole_y + px(6.25);

    let foul_ground = Data::new()
        .move_to((home_x, home_y + px(11.875)))
        .cubic_curve_to((
            home_x - px(16.875),
            home_y + px(10.625),
            home_x - px(49.375),
            home_y - px(38.125),
            left_pole_x,
            left_pole_y + px(6.25),
        ))
        .cubic_curve_to((
            home_x - px(64.0625),
            home_y - px(103.125),
            home_x - px(35.9375),
            home_y - px(121.25),
            center_fence_x,
            center_fence_y,
        ))
        .cubic_curve_to((
            home_x + px(35.9375),
            home_y - px(121.25),
            home_x + px(64.0625),
            home_y - px(103.125),
            right_pole_x,
            right_pole_y + px(6.25),
        ))
        .cubic_curve_to((
            home_x + px(49.375),
            home_y - px(38.125),
            home_x + px(16.875),
            home_y + px(10.625),
            home_x,
            home_y + px(11.875),
        ))
        .close();

    document = document.add(
        Path::new()
            .set("d", foul_ground)
            .set("fill", "#666666")
            .set("stroke", "none"),
    );

    // 3. Fair territory (infield and outfield grass)
    let fair_data = Data::new()
        .move_to((home_x, home_y))
        .line_to((left_pole_x, left_pole_y))
        .cubic_curve_to((
            home_x - px(64.0625),
            home_y - px(103.125),
            home_x - px(35.9375),
            home_y - px(121.25),
            center_fence_x,
            center_fence_y,
        ))
        .cubic_curve_to((
            home_x + px(35.9375),
            home_y - px(121.25),
            home_x + px(64.0625),
            home_y - px(103.125),
            right_pole_x,
            right_pole_y,
        ))
        .line_to((home_x, home_y))
        .close();

    document = document.add(
        Path::new()
            .set("d", fair_data)
            .set("fill", "#2e8b57")
            .set("stroke", "none"),
    );

    // 4. Infield dirt (area where infield flies are hit)
    let infield_line_offset = px(10.0);
    let infield_right_x = p1b_x + infield_line_offset;
    let infield_edge_y = p1b_y - infield_line_offset;
    let infield_left_x = p3b_x - infield_line_offset;
    let infield_data = Data::new()
        .move_to((home_x, home_y)) // Start from home base
        .line_to((infield_right_x, infield_edge_y))
        .elliptical_arc_to((px(31.0), px(31.0), 0, 0, 0, infield_left_x, infield_edge_y))
        .line_to((home_x, home_y))
        .close();

    document = document.add(
        Path::new()
            .set("d", infield_data)
            .set("fill", "#c9a56f")
            .set("stroke", "none"),
    );

    let left_foul_border = Data::new()
        .move_to((home_x, home_y + px(11.875)))
        .cubic_curve_to((
            home_x - px(16.875),
            home_y + px(10.625),
            home_x - px(49.375),
            home_y - px(38.125),
            left_pole_x,
            left_fence_y,
        ));

    document = document.add(
        Path::new()
            .set("d", left_foul_border)
            .set("fill", "none")
            .set("stroke", "#f8f8f8")
            .set("stroke-width", 7)
            .set("stroke-linecap", "round"),
    );

    let right_foul_border = Data::new()
        .move_to((home_x, home_y + px(11.875)))
        .cubic_curve_to((
            home_x + px(16.875),
            home_y + px(10.625),
            home_x + px(49.375),
            home_y - px(38.125),
            right_pole_x,
            right_fence_y,
        ));

    document = document.add(
        Path::new()
            .set("d", right_foul_border)
            .set("fill", "none")
            .set("stroke", "#f8f8f8")
            .set("stroke-width", 7)
            .set("stroke-linecap", "round"),
    );

    // 5. Outfield fence (connected to the foul line ends)
    let fence = Data::new()
        .move_to((left_pole_x, left_fence_y))
        .cubic_curve_to((
            home_x - px(64.0625),
            home_y - px(103.125),
            home_x - px(35.9375),
            home_y - px(121.25),
            center_fence_x,
            center_fence_y,
        ))
        .cubic_curve_to((
            home_x + px(35.9375),
            home_y - px(121.25),
            home_x + px(64.0625),
            home_y - px(103.125),
            right_pole_x,
            right_fence_y,
        ));

    document = document.add(
        Path::new()
            .set("id", "fence")
            .set("d", fence)
            .set("fill", "none")
            .set("stroke", "#f8f8f8")
            .set("stroke-width", 11)
            .set("stroke-linejoin", "round")
            .set("stroke-linecap", "round"),
    );

    // Foul lines (extended to the fence)
    document = document.add(
        svgLine::new()
            .set("x1", home_x)
            .set("y1", home_y)
            .set("x2", left_pole_x + px(0.625))
            .set("y2", left_pole_y + px(0.625))
            .set("stroke", "#fff")
            .set("stroke-width", 5),
    );
    document = document.add(
        svgLine::new()
            .set("x1", home_x)
            .set("y1", home_y)
            .set("x2", right_pole_x - px(0.625))
            .set("y2", right_pole_y + px(0.625))
            .set("stroke", "#fff")
            .set("stroke-width", 5),
    );
    document = document.add(
        svgLine::new()
            .set("x1", p1b_x)
            .set("y1", p1b_y)
            .set("x2", p2b_x - px(0.25))
            .set("y2", p2b_y - px(0.25))
            .set("stroke", "#fff")
            .set("stroke-width", 5),
    );
    document = document.add(
        svgLine::new()
            .set("x1", p2b_x + px(0.25))
            .set("y1", p2b_y - px(0.25))
            .set("x2", p3b_x)
            .set("y2", p3b_y)
            .set("stroke", "#fff")
            .set("stroke-width", 5),
    );

    // Bases
    let base_length = px(4.0);
    let p1b_path = Data::new()
        .move_to((p1b_x, p1b_y)) // Start from home base
        .line_to((p1b_x - base_length / 2.0, p1b_y - base_length / 2.0))
        .line_to((p1b_x - base_length, p1b_y))
        .line_to((p1b_x - base_length / 2.0, p1b_y + base_length / 2.0))
        .line_to((p1b_x, p1b_y))
        .close();

    document = document.add(
        Path::new()
            .set("d", p1b_path)
            .set("fill", "white")
            .set("stroke", "none"),
    );

    let p2b_path = Data::new()
        .move_to((p2b_x, p2b_y)) // Start from home base
        .line_to((p2b_x - base_length / 2.0, p2b_y + base_length / 2.0))
        .line_to((p2b_x, p2b_y + base_length))
        .line_to((p2b_x + base_length / 2.0, p2b_y + base_length / 2.0))
        .line_to((p2b_x, p2b_y))
        .close();

    document = document.add(
        Path::new()
            .set("d", p2b_path)
            .set("fill", "white")
            .set("stroke", "none"),
    );

    let p3b_path = Data::new()
        .move_to((p3b_x, p3b_y)) // Start from home base
        .line_to((p3b_x + base_length / 2.0, p3b_y - base_length / 2.0))
        .line_to((p3b_x + base_length, p3b_y))
        .line_to((p3b_x + base_length / 2.0, p3b_y + base_length / 2.0))
        .line_to((p3b_x, p3b_y))
        .close();

    document = document.add(
        Path::new()
            .set("d", p3b_path)
            .set("fill", "white")
            .set("stroke", "none"),
    );

    let home_base_length = px(3.5);
    let phb_path = Data::new()
        .move_to((home_x, home_y - px(0.5))) // Start from home base
        .line_to((home_x - home_base_length / 2.0, home_y - base_length / 2.0))
        .line_to((home_x - home_base_length / 2.0, home_y - base_length))
        .line_to((home_x + home_base_length / 2.0, home_y - base_length))
        .line_to((home_x + home_base_length / 2.0, home_y - base_length / 2.0))
        .line_to((home_x, home_y - px(0.5)))
        .close();

    document = document.add(
        Path::new()
            .set("d", phb_path)
            .set("fill", "white")
            .set("stroke", "none"),
    );

    // Players (polar coordinates)
    let players = vec![
        ("P", PolarPosition::new(MOUND_DISTANCE, 0.0)),
        ("C", PolarPosition::new(0.0, 0.0)),
        ("1B", PolarPosition::new(35.0, 33.0)),
        ("2B", PolarPosition::new(40.0, 18.0)),
        ("3B", PolarPosition::new(35.0, -33.0)),
        ("SS", PolarPosition::new(40.0, -18.0)),
        ("CF", PolarPosition::new(90.0, 0.0)),
        ("LF", PolarPosition::new(80.0, -26.0)),
        ("RF", PolarPosition::new(80.0, 26.0)),
    ];

    for (name, pos) in &players {
        let (px, py) = polar_to_svg(home_x, home_y, pos, scale);
        document = document.add(
            Circle::new()
                .set("cx", px)
                .set("cy", py)
                .set("r", 4)
                .set("fill", "#ff4444")
                .set("stroke", "#fff")
                .set("stroke-width", 1),
        );

        document = document.add(
            Text::new(*name)
                .set("x", px + 11.0)
                .set("y", py + 4.0)
                .set("fill", "white")
                .set("font-size", 12)
                .set("font-weight", "bold"),
        );
    }

    document
}

fn polar_to_svg(home_x: f64, home_y: f64, pos: &PolarPosition, scale: f64) -> (f64, f64) {
    let rad = (pos.angle as f64).to_radians();
    let dist_px = pos.distance as f64 * scale;
    // 0° points upward (negative y), positive angles go clockwise
    let x = home_x + dist_px * rad.sin();
    let y = home_y - dist_px * rad.cos();
    (x, y)
}

// Helper function to mathematically calculate the intersection of two line segments
fn line_intersection(line1: Line, line2: Line) -> Option<Point> {
    let p0 = line1.p0;
    let p1 = line1.p1;
    let q0 = line2.p0;
    let q1 = line2.p1;

    // Use cross product to determine the intersection
    let s1_x = p1.x - p0.x;
    let s1_y = p1.y - p0.y;
    let s2_x = q1.x - q0.x;
    let s2_y = q1.y - q0.y;

    let denom = s1_x * s2_y - s2_x * s1_y;
    if denom.abs() < 1e-9 {
        return None; // Parallel or overlapping — no intersection
    }

    let s = (-s1_y * (p0.x - q0.x) + s1_x * (p0.y - q0.y)) / denom;
    let t = (s2_x * (p0.y - q0.y) - s2_y * (p0.x - q0.x)) / denom;

    // If both parameters s and t are between 0.0 and 1.0, the line segments intersect
    if (0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&t) {
        // Calculate intersection coordinates
        Some(Point::new(p0.x + (t * s1_x), p0.y + (t * s1_y)))
    } else {
        None // Lines intersect as infinite lines, but outside the finite segment range
    }
}

// Return the first intersection found between a BezPath and a Line
// All elements are curves, so PathEl::LineTo(p) cases are not handled.
fn find_intersections(path: &BezPath, ray: Line) -> Option<Point> {
    let mut last_point = Point::ORIGIN;

    // Iterate over each element in the path (checking whether it's a line or Bezier curve)
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                last_point = p;
            }
            PathEl::CurveTo(p1, p2, p) => {
                let cubic = CubicBez::new(last_point, p1, p2, p);
                let mut flattened = Vec::new();
                kurbo::flatten(cubic.path_elements(0.1), 0.1, |flattened_el| {
                    flattened.push(flattened_el);
                });

                let mut segment_last = last_point;

                for flattened_el in flattened {
                    match flattened_el {
                        PathEl::MoveTo(pt) => {
                            segment_last = pt;
                        }
                        PathEl::LineTo(pt) => {
                            let sub_line = Line::new(segment_last, pt);
                            if let Some(intersect_pt) = line_intersection(sub_line, ray) {
                                return Some(intersect_pt);
                            }
                            segment_last = pt;
                        }
                        _ => {}
                    }
                }
                last_point = p;
            }
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_point_approx_eq(actual: Point, expected: Point) {
        let epsilon = 1e-9;
        assert!(
            (actual.x - expected.x).abs() < epsilon && (actual.y - expected.y).abs() < epsilon,
            "actual point {actual:?} did not approximately equal expected point {expected:?}"
        );
    }

    fn assert_f64_approx_eq(actual: f64, expected: f64) {
        let epsilon = 1e-6;
        assert!(
            (actual - expected).abs() < epsilon,
            "actual value {actual} did not approximately equal expected value {expected}"
        );
    }

    fn assert_path_approx_eq(actual: &BezPath, expected: &BezPath) {
        let actual_elements = actual.elements();
        let expected_elements = expected.elements();
        assert_eq!(actual_elements.len(), expected_elements.len());

        for (actual, expected) in actual_elements.iter().zip(expected_elements.iter()) {
            match (actual, expected) {
                (PathEl::MoveTo(actual), PathEl::MoveTo(expected))
                | (PathEl::LineTo(actual), PathEl::LineTo(expected)) => {
                    assert_point_approx_eq(*actual, *expected);
                }
                (
                    PathEl::CurveTo(actual_p1, actual_p2, actual_p),
                    PathEl::CurveTo(expected_p1, expected_p2, expected_p),
                ) => {
                    assert_point_approx_eq(*actual_p1, *expected_p1);
                    assert_point_approx_eq(*actual_p2, *expected_p2);
                    assert_point_approx_eq(*actual_p, *expected_p);
                }
                (PathEl::QuadTo(actual_p1, actual_p), PathEl::QuadTo(expected_p1, expected_p)) => {
                    assert_point_approx_eq(*actual_p1, *expected_p1);
                    assert_point_approx_eq(*actual_p, *expected_p);
                }
                (PathEl::ClosePath, PathEl::ClosePath) => {}
                _ => panic!("actual path element {actual:?} did not match expected {expected:?}"),
            }
        }
    }

    #[test]
    fn build_fence_line_json_round_trips_to_bez_path() {
        let json = Stadium::build_fence_line_json(98.0, 120.0).unwrap();
        let fence_line: BezPath = serde_json::from_str(&json).unwrap();

        assert_path_approx_eq(&fence_line, &Stadium::build_fence_line(98.0, 120.0));
    }

    #[test]
    fn fence_distance_at_angle_uses_actual_fence_line() {
        let stadium = Stadium::new(1, "Test Stadium".to_string(), 98.0, 120.0, 2.0);

        let center_distance = stadium.fence_distance_at_angle(0.0).unwrap();
        let right_field_distance = stadium.fence_distance_at_angle(45.0).unwrap();

        assert_f64_approx_eq(center_distance, 120.0);
        assert!(
            right_field_distance < center_distance,
            "right field fence distance {right_field_distance} should be shorter than center {center_distance}"
        );
    }
}
