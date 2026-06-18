use crate::domain::shared::game::BASE_DISTANCE;
use crate::domain::util::PolarPosition;
use crate::proj_dirs;
use kurbo::{Affine, BezPath, Point, Shape, Vec2};
use std::f64::consts::SQRT_2;
use svg::Document;
use svg::node::element::path::Data;
use svg::node::element::{Circle, Line, Path, Rectangle, Text};

pub const MOUND_DISTANCE: f64 = 18.44;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

pub struct Stadium {
    pub name: String,
    pub foul_pole_distance: f64,
    pub center_fence_distance: f64,
    // pub fair_zone: kurbo::BezPath,
    fence_line: kurbo::BezPath,
    // pub fence_height: f64,
}
impl Stadium {
    pub fn new(name: String, foul_pole_distance: f64, center_fence_distance: f64) -> Self {
        let homerun_pole_x = foul_pole_distance / SQRT_2;
        let homerun_pole_y = foul_pole_distance / SQRT_2;
        let center_fence_x = 0.0;
        let center_fence_y = center_fence_distance;
        let foul_fence_y = homerun_pole_y - foul_pole_distance * 0.065;

        let backnet_x1 = 0.07 * center_fence_distance;
        let backnet_y1 = 0.05 * center_fence_distance;
        let backnet_x2 = 0.14 * center_fence_distance;
        let backnet_y2 = 0.09 * center_fence_distance;
        let infield_x = 0.41 * center_fence_distance;
        let infield_y = 0.32 * center_fence_distance;
        let outfield_x1 = 0.534 * center_fence_distance;
        let outfield_y1 = 0.86 * center_fence_distance;
        let outfield_x2 = 0.3 * center_fence_distance;
        let outfield_y2 = 1.01 * center_fence_distance;

        let mut fence_line = BezPath::new();
        fence_line.move_to(Point::new(0.0, 0.0));
        fence_line.curve_to(
            Point::new(backnet_x2, backnet_y2),
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
            Point::new(-backnet_x2, backnet_y2),
            Point::new(-backnet_x1, backnet_y1),
        );
        fence_line.curve_to(
            Point::new(0.0, 0.0),
            Point::new(0.0, 0.0),
            Point::new(backnet_x1, backnet_y1),
        );
        fence_line.close_path();

        Self {
            name: name,
            foul_pole_distance: foul_pole_distance,
            center_fence_distance: center_fence_distance,
            fence_line: fence_line,
        }
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
            .set("stroke", "white") // 白線
            .set("stroke-width", 2)
            .set("d", fence_line_svg_path.to_svg());

        let fence_svg = Document::new()
            .set("viewBox", (0, 0, svg_width, svg_height))
            .set("width", svg_width)
            .set("height", svg_height)
            .add(fence_path);

        draw(fence_svg);
    }

    pub fn is_inside_fence_line(&self, point: kurbo::Point) -> bool {
        self.fence_line.contains(point)
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
        Line::new()
            .set("x1", home_x)
            .set("y1", home_y)
            .set("x2", left_pole_x + px(0.625))
            .set("y2", left_pole_y + px(0.625))
            .set("stroke", "#fff")
            .set("stroke-width", 5),
    );
    document = document.add(
        Line::new()
            .set("x1", home_x)
            .set("y1", home_y)
            .set("x2", right_pole_x - px(0.625))
            .set("y2", right_pole_y + px(0.625))
            .set("stroke", "#fff")
            .set("stroke-width", 5),
    );
    document = document.add(
        Line::new()
            .set("x1", p1b_x)
            .set("y1", p1b_y)
            .set("x2", p2b_x - px(0.25))
            .set("y2", p2b_y - px(0.25))
            .set("stroke", "#fff")
            .set("stroke-width", 5),
    );
    document = document.add(
        Line::new()
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

    // svg::save("baseball_field.svg", &document).unwrap();
}

fn polar_to_svg(home_x: f64, home_y: f64, pos: &PolarPosition, scale: f64) -> (f64, f64) {
    let rad = (pos.angle as f64).to_radians();
    let dist_px = pos.distance as f64 * scale;
    // 0° points upward (negative y), positive angles go clockwise
    let x = home_x + dist_px * rad.sin();
    let y = home_y - dist_px * rad.cos();
    (x, y)
}
