//! Diagnostic adapter for recorded legacy contacts. No search, RNG or live result changes.
use super::{
    ball::PitchedBall,
    finite_bat_collision::{
        self as finite, CollisionGeometry, CollisionParameters, FiniteCollisionInput,
        PitchSpinInput,
    },
    forecast_swing_plan::{add, cross, dot, mul},
    game_stats::PlayerGameBatting,
};
use crate::domain::resolver::{
    batting_resolver::calculate_vertical_contact_deflection,
    pitching_resolver::StrikeZoneDimensions,
};
use serde::Serialize;
use std::f64::consts::PI;
type V3 = [f64; 3];
pub const SOURCE: &str = "legacy_contact_cylinder_face_proxy_v1";
const RADIUS: f64 = 0.070;
const LEVER: f64 = 0.9;
const HALF_LENGTH: f64 = 0.16;
fn norm(v: V3) -> f64 {
    dot(v, v).sqrt()
}
fn yaw(v: V3, a: f64) -> V3 {
    [
        a.cos() * v[0] - a.sin() * v[1],
        a.sin() * v[0] + a.cos() * v[1],
        v[2],
    ]
}
#[derive(Debug, Serialize)]
pub struct FrozenInput {
    pub input: FiniteCollisionInput,
    pub signed_length_m: f64,
    pub signed_thickness_m: f64,
    pub face_angle_deg: f64,
    pub attack_angle_deg: f64,
    pub reference_speed_ms: f64,
}
/// Preserve recorded local offsets. The unknown 3-D pose is an explicit proxy:
/// circular leading surface, recorded tilt/face, rigid rotation about the 0.9 m lever.
pub fn from_record(
    b: &PlayerGameBatting,
    pitch: &PitchedBall,
) -> Result<FrozenInput, &'static str> {
    if b.hla_model_version != Some(2) {
        return Err("unsupported_hla_model");
    }
    let tilt = b
        .actual_bat_angle_deg
        .ok_or("missing_bat_tilt")?
        .to_radians();
    let x = b.contact_offset_x_m.ok_or("missing_contact_x")?;
    let z = b.contact_offset_z_m.ok_or("missing_contact_z")?;
    let sign = b.hla_handedness_sign.ok_or("missing_handedness")?;
    let face = b.approach_variation_deg + b.pull_bias_deg + b.timing_angle_error;
    let attack = b.ball.launch_angle - calculate_vertical_contact_deflection(z);
    if ![
        tilt,
        x,
        z,
        sign,
        face,
        attack,
        b.bat_speed,
        pitch.flight_time,
    ]
    .iter()
    .all(|v| v.is_finite())
        || ![-1.0, 1.0].contains(&sign)
        || b.bat_speed <= 0.0
        || attack.abs() > 90.0
        || pitch.flight_time <= 0.0
    {
        return Err("invalid_contact_pose");
    }
    let length = x * tilt.cos() + z * tilt.sin();
    let thickness = -x * tilt.sin() + z * tilt.cos();
    for (recorded, expected) in [
        (b.signed_length_offset_m, length),
        (b.signed_thickness_offset_m, thickness),
    ] {
        if recorded.is_some_and(|v| !v.is_finite() || (v - expected).abs() > 1e-8) {
            return Err("signed_offset_mismatch");
        }
    }
    if length.abs() > HALF_LENGTH {
        return Err("unsupported_cap");
    }
    if thickness.abs() > RADIUS {
        return Err("outside_contact_radius");
    }
    let e = finite::profile_e(0.71 + length).ok_or("out_of_restitution_curve")?;
    let radial = thickness / RADIUS;
    let angle = -sign * face.to_radians();
    let axis = yaw([sign * tilt.cos(), 0.0, tilt.sin()], angle);
    let normal = yaw(
        [
            -sign * tilt.sin() * radial,
            (1.0 - radial * radial).max(0.0).sqrt(),
            tilt.cos() * radial,
        ],
        angle,
    );
    let reference_velocity = [
        0.0,
        b.bat_speed * attack.to_radians().cos(),
        b.bat_speed * attack.to_radians().sin(),
    ];
    let omega = mul(cross(axis, reference_velocity), 1.0 / LEVER);
    let centerline = add(reference_velocity, cross(omega, mul(axis, length)));
    let vin = pitch
        .velocity_at_plate(&StrikeZoneDimensions::default())
        .ok_or("invalid_pitch_velocity")?;
    let vin = [vin.x, vin.y, vin.z];
    // Gyro sign is not recorded. Match the existing diagnostic convention: omit gyro.
    let spin = finite::reconstruct_spin(
        PitchSpinInput {
            spin_rate: Some(pitch.spin_rate),
            spin_angle: Some(pitch.spin_angle),
            spin_efficiency: Some(pitch.spin_efficiency),
            gyro_sign: 0,
        },
        vin,
    )?;
    let geometry = CollisionGeometry {
        time_since_release_sec: pitch.flight_time,
        axis_world: axis,
        normal_world: normal,
        axial_center_m: length,
        closest_axial_m: length,
        half_length_m: HALF_LENGTH,
        region: "side".into(),
    };
    geometry.validate()?;
    Ok(FrozenInput {
        input: FiniteCollisionInput {
            geometry,
            vin,
            centerline_velocity: centerline,
            ball_omega: spin,
            bat_omega: omega,
            parameters: CollisionParameters::frozen(e, RADIUS),
        },
        signed_length_m: length,
        signed_thickness_m: thickness,
        face_angle_deg: face,
        attack_angle_deg: attack,
        reference_speed_ms: b.bat_speed,
    })
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct FlightSpin {
    pub total_rpm: f64,
    pub transverse_rpm: f64,
    pub gyro_rpm: f64,
    pub angle_deg: f64,
}
/// Project omega into the flight integrator's lift/right basis. Gyro has no Magnus effect.
pub fn flight_spin(velocity: V3, omega: V3) -> Result<FlightSpin, &'static str> {
    if !velocity.iter().chain(omega.iter()).all(|v| v.is_finite()) || norm(velocity) <= 1e-12 {
        return Err("degenerate_outgoing_velocity");
    }
    let u = mul(velocity, 1.0 / norm(velocity));
    let horizontal = u[0].hypot(u[1]);
    if horizontal <= 1e-12 {
        return Err("vertical_spin_frame");
    }
    let up = [
        -u[2] * u[0] / horizontal,
        -u[2] * u[1] / horizontal,
        horizontal,
    ];
    let right = cross(u, up);
    let lift = cross(omega, u);
    let transverse = norm(lift);
    let angle = if transverse <= 1e-12 {
        0.0
    } else {
        dot(lift, right)
            .atan2(dot(lift, up))
            .to_degrees()
            .rem_euclid(360.0)
    };
    Ok(FlightSpin {
        total_rpm: norm(omega) * 30.0 / PI,
        transverse_rpm: transverse * 30.0 / PI,
        gyro_rpm: dot(omega, u).abs() * 30.0 / PI,
        angle_deg: angle,
    })
}
/// Same geometry/inertia/restitution in all cases; only motion and tangential response vary.
pub fn case_input(input: &FiniteCollisionInput, case: &str) -> FiniteCollisionInput {
    let mut v = input.clone();
    if case == "stationary_normal" {
        v.centerline_velocity = [0.0; 3];
        v.bat_omega = [0.0; 3];
    }
    if case != "moving_finite" {
        v.parameters.mu = 0.0;
        v.parameters.et = 0.0;
    }
    v
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample(x: f64, z: f64, sign: f64) -> (PlayerGameBatting, PitchedBall) {
        use crate::domain::{
            shared::{
                ball::{BallLocation, BattedBall},
                game::{BattingResult, GameResult},
                game_stats::PlayerGameBattingMetrics,
                player::PitchType,
            },
            strategy::pitching_strategy::TargetZone,
            util::Vector3D,
        };
        let mut game = GameResult::new(
            1,
            chrono::NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
            &[],
            &[],
        );
        let mut ball = BattedBall::default();
        ball.launch_angle = 10.0 + calculate_vertical_contact_deflection(z);
        let tilt = 30.0_f64.to_radians();
        let metrics = PlayerGameBattingMetrics {
            hla_model_version: Some(2),
            hla_handedness_sign: Some(sign),
            actual_bat_angle_deg: Some(30.0),
            contact_offset_x_m: Some(x),
            contact_offset_z_m: Some(z),
            signed_length_offset_m: Some(x * tilt.cos() + z * tilt.sin()),
            signed_thickness_offset_m: Some(-x * tilt.sin() + z * tilt.cos()),
            bat_speed: 35.0,
            approach_variation_deg: 2.0,
            pull_bias_deg: 1.0,
            timing_angle_error: 3.0,
            ..Default::default()
        };
        game.add_player_batting(1, 10, 7, metrics, ball, None, BattingResult::Single);
        let pitch = PitchedBall {
            pitch_type: PitchType::FourSeamFastball,
            speed: 40.0,
            spin_rate: 2200.0,
            spin_angle: 0.0,
            spin_efficiency: 0.9,
            release_point: Vector3D {
                x: 0.0,
                y: 16.0,
                z: 1.8,
            },
            flight_time: 0.4,
            aim_zone: TargetZone::Center,
            aim_location: BallLocation { x: 0.0, y: 0.0 },
            actual_location: BallLocation { x: 0.0, y: 0.0 },
        };
        (game.player_battings[0], pitch)
    }
    #[test]
    fn frozen_adapter_preserves_offsets_and_mirrors_the_full_response() {
        let (right, pitch) = sample(0.02, 0.01, 1.0);
        let (left, _) = sample(0.02, 0.01, -1.0);
        let a = from_record(&right, &pitch).unwrap();
        let b = from_record(&left, &pitch).unwrap();
        assert!((a.attack_angle_deg - 10.0).abs() < 1e-10);
        assert_eq!(a.signed_length_m, right.signed_length_offset_m.unwrap());
        assert_eq!(
            a.signed_thickness_m,
            right.signed_thickness_offset_m.unwrap()
        );
        let ar = finite::solve(&a.input).unwrap();
        let br = finite::solve(&b.input).unwrap();
        assert_eq!(ar["collision_status"], "impulse_contact");
        for k in ["ev_ms", "vla_deg", "spin_rpm"] {
            assert!((ar[k].as_f64().unwrap() - br[k].as_f64().unwrap()).abs() < 1e-8);
        }
        assert!((ar["hla_deg"].as_f64().unwrap() + br["hla_deg"].as_f64().unwrap()).abs() < 1e-8);
        assert_eq!(right.hla_handedness_sign, Some(1.0));
    }
    #[test]
    fn adapter_rejects_missing_inputs_caps_and_out_of_curve_without_clamping() {
        let (mut b, pitch) = sample(0.0, 0.0, 1.0);
        b.signed_length_offset_m = Some(0.001);
        assert_eq!(
            from_record(&b, &pitch).unwrap_err(),
            "signed_offset_mismatch"
        );
        b.contact_offset_x_m = None;
        assert_eq!(from_record(&b, &pitch).unwrap_err(), "missing_contact_x");
        // x/cos(tilt) produces a desired axial offset with zero thickness.
        let (b, pitch) = sample(0.14 * 30.0_f64.to_radians().cos(), 0.14 * 0.5, 1.0);
        assert_eq!(
            from_record(&b, &pitch).unwrap_err(),
            "out_of_restitution_curve"
        );
        let (b, pitch) = sample(0.17 * 30.0_f64.to_radians().cos(), 0.17 * 0.5, 1.0);
        assert_eq!(from_record(&b, &pitch).unwrap_err(), "unsupported_cap");
    }
    #[test]
    fn gyro_spin_is_excluded_from_magnus_and_mirroring_preserves_magnitudes() {
        assert_eq!(
            flight_spin([0.0, 40.0, 0.0], [10.0, 0.0, 0.0])
                .unwrap()
                .angle_deg,
            0.0
        );
        assert_eq!(
            flight_spin([0.0, 40.0, 0.0], [0.0, 0.0, -10.0])
                .unwrap()
                .angle_deg,
            90.0
        );
        let s = flight_spin([0.0, 40.0, 0.0], [10.0, 20.0, 30.0]).unwrap();
        assert!((s.transverse_rpm - 10.0_f64.hypot(30.0) * 30.0 / PI).abs() < 1e-10);
        assert!((s.gyro_rpm - 20.0 * 30.0 / PI).abs() < 1e-10);
        let mirror = flight_spin([0.0, 40.0, 0.0], [10.0, -20.0, -30.0]).unwrap();
        assert!((mirror.total_rpm - s.total_rpm).abs() < 1e-10);
        assert!((mirror.transverse_rpm - s.transverse_rpm).abs() < 1e-10);
        assert!((mirror.angle_deg - (360.0 - s.angle_deg)).abs() < 1e-10);
        assert_eq!(
            flight_spin([0.0, 40.0, 0.0], [0.0, 100.0, 0.0])
                .unwrap()
                .transverse_rpm,
            0.0
        );
        assert!(flight_spin([0.0; 3], [0.0; 3]).is_err());
    }
    #[test]
    fn moving_normal_control_matches_analytic_finite_mass_impulse() {
        let geometry = CollisionGeometry {
            time_since_release_sec: 0.4,
            axis_world: [1.0, 0.0, 0.0],
            normal_world: [0.0, 1.0, 0.0],
            axial_center_m: 0.0,
            closest_axial_m: 0.0,
            half_length_m: HALF_LENGTH,
            region: "side".into(),
        };
        let input = FiniteCollisionInput {
            geometry,
            vin: [0.0, -40.0, 0.0],
            centerline_velocity: [0.0, 35.0, 0.0],
            ball_omega: [0.0; 3],
            bat_omega: [0.0; 3],
            parameters: CollisionParameters::frozen(0.5, RADIUS),
        };
        let result = finite::solve(&case_input(&input, "moving_normal")).unwrap();
        let p = input.parameters;
        let inverse = 1.0 / p.mass + p.cm_offset.powi(2) / p.transverse_inertia;
        let expected = -40.0 + 1.5 * 75.0 / (1.0 + p.ball_mass * inverse);
        assert!((result["outgoing_world_ms"][1].as_f64().unwrap() - expected).abs() < 1e-10);
        let stationary = finite::solve(&case_input(&input, "stationary_normal")).unwrap();
        assert!(stationary["ev_ms"].as_f64().unwrap() < result["ev_ms"].as_f64().unwrap());
        assert_eq!(input.centerline_velocity, [0.0, 35.0, 0.0]);
    }
}
