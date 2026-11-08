//! Diagnostic flight controls. Published coefficients are measurement-fitted, not a new live model.
use crate::domain::{
    resolver::batting_resolver::LIVE_FLIGHT_DRAG_PER_M, shared::ball::MAGNUS_COEFF, util::GRAVITY,
};
use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
pub const SOURCE_URL: &str = "https://baseball.physics.illinois.edu/TrajectoryAnalysis.pdf";
pub const MASS_KG: f64 = 5.125 * 0.028349523125;
pub const RADIUS_M: f64 = 9.125 * 0.0254 / (2.0 * PI);
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Launch {
    pub speed_ms: f64,
    pub vla_deg: f64,
    pub hla_deg: f64,
    pub spin_rpm: f64,
    pub spin_angle_deg: f64,
    pub height_m: f64,
    pub wind_speed_ms: f64,
    pub wind_direction_deg: f64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForceModel {
    Vacuum,
    Live,
    Published2017,
    PublishedDragLiveLift,
    LiveDragPublishedLift,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub dt_sec: f64,
    pub max_time_sec: f64,
    pub density_kg_m3: f64,
    pub spin_decay_tau_sec: Option<f64>,
    pub marker_y_m: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            dt_sec: 0.001,
            max_time_sec: 30.0,
            density_kg_m3: 1.194,
            spin_decay_tau_sec: None,
            marker_y_m: 120.0,
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Flight {
    pub carry_m: f64,
    pub landing_position_m: [f64; 3],
    pub landing_time_sec: f64,
    pub max_height_m: f64,
    pub marker_crossing_height_m: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct ForceComparison {
    pub speed_ms: f64,
    pub spin_rpm: f64,
    pub spin_factor: f64,
    pub published_cd: f64,
    pub published_cl: f64,
    pub live_equivalent_cd: f64,
    pub live_equivalent_cl: f64,
    pub published_drag_ms2: f64,
    pub published_lift_ms2: f64,
    pub live_drag_ms2: f64,
    pub live_lift_ms2: f64,
}
/// Eq. 10-11, Nathan (2017). Nominal mass/circumference; density matches its indoor data.
pub fn coefficients(speed: f64, spin_rpm: f64) -> (f64, f64) {
    let s = if speed > 0.0 {
        RADIUS_M * spin_rpm * PI / 30.0 / speed
    } else {
        0.0
    };
    (
        0.297 + 0.0292 * spin_rpm / 1000.0,
        1.120 * s / (0.583 + 2.333 * s),
    )
}
pub fn force_comparison(
    speed: f64,
    spin_rpm: f64,
    density: f64,
) -> Result<ForceComparison, &'static str> {
    if ![speed, spin_rpm, density].iter().all(|x| x.is_finite())
        || speed <= 0.0
        || spin_rpm < 0.0
        || density <= 0.0
    {
        return Err("invalid_force_input");
    }
    let k = 0.5 * density * PI * RADIUS_M.powi(2) / MASS_KG;
    let (cd, cl) = coefficients(speed, spin_rpm);
    let ld = LIVE_FLIGHT_DRAG_PER_M * speed * speed;
    let ll = if speed > 0.1 && spin_rpm > 10.0 {
        MAGNUS_COEFF * spin_rpm * speed
    } else {
        0.0
    };
    Ok(ForceComparison {
        speed_ms: speed,
        spin_rpm,
        spin_factor: RADIUS_M * spin_rpm * PI / 30.0 / speed,
        published_cd: cd,
        published_cl: cl,
        live_equivalent_cd: LIVE_FLIGHT_DRAG_PER_M / k,
        live_equivalent_cl: ll / (k * speed * speed),
        published_drag_ms2: k * cd * speed * speed,
        published_lift_ms2: k * cl * speed * speed,
        live_drag_ms2: ld,
        live_lift_ms2: ll,
    })
}
fn basis(u: DVec3) -> (DVec3, DVec3) {
    let raw = DVec3::Z - u * u.z;
    let up = if raw.length() > 0.001 {
        raw.normalize()
    } else {
        DVec3::Z
    };
    (up, u.cross(up))
}
fn initial(l: Launch) -> (DVec3, DVec3) {
    let a = l.vla_deg.to_radians();
    let h = l.hla_deg.to_radians();
    let v = DVec3::new(
        l.speed_ms * a.cos() * h.sin(),
        l.speed_ms * a.cos() * h.cos(),
        l.speed_ms * a.sin(),
    );
    let u = v.normalize();
    let (mut up, _) = basis(u);
    if up.cross(u).length() < 1e-10 {
        up = DVec3::X;
    }
    let lift = up * l.spin_angle_deg.to_radians().cos()
        + u.cross(up) * l.spin_angle_deg.to_radians().sin();
    (v, u.cross(lift) * l.spin_rpm * PI / 30.0)
}
fn acceleration(
    v: DVec3,
    t: f64,
    l: Launch,
    s: Settings,
    model: ForceModel,
    omega0: DVec3,
) -> DVec3 {
    if matches!(model, ForceModel::Vacuum) {
        return -DVec3::Z * GRAVITY;
    }
    let wind = DVec3::new(
        l.wind_speed_ms * l.wind_direction_deg.to_radians().sin(),
        l.wind_speed_ms * l.wind_direction_deg.to_radians().cos(),
        0.0,
    );
    let air = v - wind;
    let speed = air.length();
    if speed <= 1e-12 {
        return -DVec3::Z * GRAVITY;
    }
    let decay = s.spin_decay_tau_sec.map_or(1.0, |tau| (-t / tau).exp());
    let omega = omega0 * decay;
    let rpm = l.spin_rpm * decay;
    let k = 0.5 * s.density_kg_m3 * PI * RADIUS_M.powi(2) / MASS_KG;
    let (cd, cl) = coefficients(speed, rpm);
    let drag = if matches!(model, ForceModel::Live | ForceModel::LiveDragPublishedLift) {
        LIVE_FLIGHT_DRAG_PER_M
    } else {
        k * cd
    };
    let lift = if matches!(model, ForceModel::Live | ForceModel::PublishedDragLiveLift) {
        if speed > 0.1 && rpm > 10.0 {
            let (up, right) = basis(air / speed);
            let a = l.spin_angle_deg.to_radians();
            (up * a.cos() + right * a.sin()) * MAGNUS_COEFF * rpm * speed
        } else {
            DVec3::ZERO
        }
    } else {
        let cross = omega.cross(air / speed);
        // Constant world spin axis as in the published model; only transverse lift contributes.
        if omega.length() > 1e-12 {
            cross / omega.length() * k * cl * speed * speed
        } else {
            DVec3::ZERO
        }
    };
    -air * drag * speed + lift - DVec3::Z * GRAVITY
}
/// Independent RK4 free flight, stops at first ground crossing; no wall, bounce or rolling.
pub fn integrate(l: Launch, s: Settings, model: ForceModel) -> Result<Flight, &'static str> {
    if ![
        l.speed_ms,
        l.vla_deg,
        l.hla_deg,
        l.spin_rpm,
        l.spin_angle_deg,
        l.height_m,
        l.wind_speed_ms,
        l.wind_direction_deg,
        s.dt_sec,
        s.max_time_sec,
        s.density_kg_m3,
        s.marker_y_m,
    ]
    .iter()
    .all(|x| x.is_finite())
        || l.speed_ms <= 0.0
        || l.height_m <= 0.0
        || l.spin_rpm < 0.0
        || l.wind_speed_ms < 0.0
        || l.vla_deg.abs() > 90.0
        || s.max_time_sec / s.dt_sec > 2_000_000.0
        || s.dt_sec <= 0.0
        || s.max_time_sec <= 0.0
        || s.density_kg_m3 <= 0.0
        || s.marker_y_m <= 0.0
        || s.spin_decay_tau_sec
            .is_some_and(|v| !v.is_finite() || v <= 0.0)
    {
        return Err("invalid_flight_input");
    }
    let (mut v, omega) = initial(l);
    let mut p = DVec3::new(0.0, 0.0, l.height_m);
    let mut t = 0.0;
    let mut maximum = l.height_m;
    let mut marker = None;
    while t < s.max_time_sec {
        let h = s.dt_sec.min(s.max_time_sec - t);
        if t + h <= t {
            return Err("time_step_stalled");
        }
        let a1 = acceleration(v, t, l, s, model, omega);
        let v2 = v + a1 * (h / 2.0);
        let a2 = acceleration(v2, t + h / 2.0, l, s, model, omega);
        let v3 = v + a2 * (h / 2.0);
        let a3 = acceleration(v3, t + h / 2.0, l, s, model, omega);
        let v4 = v + a3 * h;
        let a4 = acceleration(v4, t + h, l, s, model, omega);
        let next = p + (v + v2 * 2.0 + v3 * 2.0 + v4) * (h / 6.0);
        let nv = v + (a1 + a2 * 2.0 + a3 * 2.0 + a4) * (h / 6.0);
        if !next.is_finite() || !nv.is_finite() {
            return Err("non_finite_flight");
        }
        maximum = maximum.max(next.z);
        if marker.is_none() && p.y < s.marker_y_m && next.y >= s.marker_y_m {
            let fraction = (s.marker_y_m - p.y) / (next.y - p.y);
            let z = p.z + fraction * (next.z - p.z);
            if z >= 0.0 {
                marker = Some(z);
            }
        }
        if next.z <= 0.0 {
            let fraction = p.z / (p.z - next.z);
            let land = p + (next - p) * fraction;
            return Ok(Flight {
                carry_m: land.x.hypot(land.y),
                landing_position_m: [land.x, land.y, 0.0],
                landing_time_sec: t + h * fraction,
                max_height_m: maximum,
                marker_crossing_height_m: marker,
            });
        }
        p = next;
        v = nv;
        t += h;
    }
    Err("no_ground_event_before_time_limit")
}
#[cfg(test)]
mod tests {
    use super::*;
    fn same(a: &serde_json::Value, b: &serde_json::Value) {
        match (a, b) {
            (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
                assert_eq!(a.len(), b.len());
                for (k, v) in a {
                    same(v, &b[k]);
                }
            }
            (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
                assert_eq!(a.len(), b.len());
                for (a, b) in a.iter().zip(b) {
                    same(a, b);
                }
            }
            (serde_json::Value::Number(a), serde_json::Value::Number(b)) => assert!(
                (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 0.002,
                "{a} != {b}"
            ),
            _ => assert_eq!(a, b),
        }
    }
    #[test]
    fn independent_scipy_ground_event_fixtures_match() {
        let v: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/flight_reference_contract.json"
        ))
        .unwrap();
        for c in v["cases"].as_array().unwrap() {
            let l: Launch = serde_json::from_value(c["launch"].clone()).unwrap();
            let s: Settings = serde_json::from_value(c["settings"].clone()).unwrap();
            let m: ForceModel = serde_json::from_value(c["model"].clone()).unwrap();
            let out = integrate(l, s, m).unwrap();
            same(&c["expected"], &serde_json::to_value(out).unwrap());
        }
    }
    #[test]
    fn vacuum_matches_analytic_landing_and_step_convergence() {
        let l = Launch {
            speed_ms: 45.0,
            vla_deg: 30.0,
            hla_deg: 0.0,
            spin_rpm: 2600.0,
            spin_angle_deg: 0.0,
            height_m: 0.9,
            wind_speed_ms: 0.0,
            wind_direction_deg: 0.0,
        };
        let vz = l.speed_ms * l.vla_deg.to_radians().sin();
        let expected = (vz + (vz * vz + 2.0 * GRAVITY * l.height_m).sqrt()) / GRAVITY;
        let a = integrate(l, Settings::default(), ForceModel::Vacuum).unwrap();
        assert!((a.landing_time_sec - expected).abs() < 1e-6);
        assert!((a.carry_m - l.speed_ms * l.vla_deg.to_radians().cos() * expected).abs() < 1e-4);
        let a = integrate(l, Settings::default(), ForceModel::Published2017).unwrap();
        let b = integrate(
            l,
            Settings {
                dt_sec: 0.0005,
                ..Settings::default()
            },
            ForceModel::Published2017,
        )
        .unwrap();
        assert!((a.carry_m - b.carry_m).abs() < 0.0001);
    }
    #[test]
    fn inputs_and_reaching_time_limit_are_explicit_failures() {
        assert!(force_comparison(0.0, 2600.0, 1.194).is_err());
        assert_eq!(
            force_comparison(45.0, 5.0, 1.194).unwrap().live_lift_ms2,
            0.0
        );
        let l = Launch {
            speed_ms: 45.0,
            vla_deg: 30.0,
            hla_deg: 0.0,
            spin_rpm: 0.0,
            spin_angle_deg: 0.0,
            height_m: -0.1,
            wind_speed_ms: 0.0,
            wind_direction_deg: 0.0,
        };
        assert_eq!(
            integrate(l, Settings::default(), ForceModel::Live).unwrap_err(),
            "invalid_flight_input"
        );
        assert_eq!(
            integrate(
                Launch { height_m: 0.9, ..l },
                Settings {
                    max_time_sec: 0.01,
                    ..Settings::default()
                },
                ForceModel::Live
            )
            .unwrap_err(),
            "no_ground_event_before_time_limit"
        );
    }
}
