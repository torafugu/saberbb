//! Database-free fixed-launch flight comparison. Outputs JSON; no gameplay coefficient changes.
use anyhow::{Result, ensure};
use clap::Parser;
use saberbb::domain::{
    resolver::batting_resolver::{
        LIVE_FLIGHT_DT_SEC, TrajectoryEnvironment, calculate_trajectory_with_environment,
    },
    shared::{
        flight_drag::FlightDragModel,
        flight_reference::{self as reference, ForceModel, Launch, Settings},
        stadium::Stadium,
    },
};
use serde_json::json;
#[derive(Parser)]
struct Args {
    #[arg(long, value_delimiter = ',', default_value = "40.2336,45,49.1744")]
    speeds_ms: Vec<f64>,
    #[arg(long, value_delimiter = ',', default_value = "20,30,35")]
    vla_deg: Vec<f64>,
    #[arg(long, value_delimiter = ',', default_value = "0,400,2600,4000")]
    spin_rpm: Vec<f64>,
    #[arg(long, default_value_t = 0.001)]
    dt_sec: f64,
    #[arg(long, default_value_t = 0.9)]
    height_m: f64,
    #[arg(long, default_value_t = 0.0)]
    wind_speed_ms: f64,
    #[arg(long, default_value_t = 0.0)]
    wind_direction_deg: f64,
}
fn main() -> Result<()> {
    let a = Args::parse();
    ensure!(
        !a.speeds_ms.is_empty() && !a.vla_deg.is_empty() && !a.spin_rpm.is_empty(),
        "empty input grid"
    );
    ensure!(
        a.speeds_ms.iter().all(|x| x.is_finite() && *x > 0.0)
            && a.vla_deg.iter().all(|x| x.is_finite() && x.abs() <= 90.0)
            && a.spin_rpm.iter().all(|x| x.is_finite() && *x >= 0.0),
        "invalid launch grid"
    );
    ensure!(
        [a.dt_sec, a.height_m, a.wind_speed_ms, a.wind_direction_deg]
            .iter()
            .all(|x| x.is_finite())
            && a.dt_sec > 0.0
            && 30.0 / a.dt_sec <= 2_000_000.0
            && a.dt_sec <= 0.05
            && a.height_m > 0.0
            && a.wind_speed_ms >= 0.0,
        "invalid environment/step"
    );
    let settings = Settings {
        dt_sec: a.dt_sec,
        ..Default::default()
    };
    let wide = Stadium::new(1, "diagnostic wide field".into(), 500.0, 500.0, 2.0);
    let mut rows = Vec::new();
    for &speed in &a.speeds_ms {
        for &vla in &a.vla_deg {
            for &spin in &a.spin_rpm {
                let l = Launch {
                    speed_ms: speed,
                    vla_deg: vla,
                    hla_deg: 0.0,
                    spin_rpm: spin,
                    spin_angle_deg: 0.0,
                    height_m: a.height_m,
                    wind_speed_ms: a.wind_speed_ms,
                    wind_direction_deg: a.wind_direction_deg,
                };
                let env = TrajectoryEnvironment {
                    impact_height_m: a.height_m,
                    wind_speed_ms: a.wind_speed_ms,
                    wind_direction_deg: a.wind_direction_deg,
                    stop_at_first_ground: true,
                    ..TrajectoryEnvironment::for_stadium(&wide)
                };
                let mut models = serde_json::Map::new();
                for (name, drag_model) in [
                    ("live_discrete", FlightDragModel::Legacy),
                    ("published_drag_discrete", FlightDragModel::Published2017),
                ] {
                    let live = match calculate_trajectory_with_environment(
                        speed,
                        vla,
                        0.0,
                        spin,
                        0.0,
                        &wide,
                        TrajectoryEnvironment { drag_model, ..env },
                    ) {
                        Ok(b) if b.first_bounce_position.is_some() => {
                            json!({"status":"landed","carry_m":b.first_bounce_position.unwrap().distance,"landing_time_sec":b.first_bounce_time,"max_height_m":b.max_height})
                        }
                        Ok(b) => {
                            json!({"status":"no_ground_before_boundary","outbound":b.outbound_result})
                        }
                        Err(e) => json!({"status":"error","reason":e.to_string()}),
                    };
                    models.insert(name.into(), live);
                }
                for (name, model, tau) in [
                    ("vacuum_rk4", ForceModel::Vacuum, None),
                    ("live_forces_rk4", ForceModel::Live, None),
                    ("published2017", ForceModel::Published2017, None),
                    (
                        "published_drag_live_lift",
                        ForceModel::PublishedDragLiveLift,
                        None,
                    ),
                    (
                        "live_drag_published_lift",
                        ForceModel::LiveDragPublishedLift,
                        None,
                    ),
                    (
                        "published2017_tau30_sensitivity",
                        ForceModel::Published2017,
                        Some(30.0),
                    ),
                ] {
                    let value = match reference::integrate(
                        l,
                        Settings {
                            spin_decay_tau_sec: tau,
                            ..settings
                        },
                        model,
                    ) {
                        Ok(result) => {
                            let mut v = serde_json::to_value(&result)?;
                            v["status"] = json!("landed");
                            v["clears_120m_2m_marker"] =
                                json!(result.marker_crossing_height_m.is_some_and(|z| z > 2.0));
                            v
                        }
                        Err(reason) => json!({"status":"error","reason":reason}),
                    };
                    models.insert(name.into(), value);
                }
                rows.push(json!({"launch":l,"within_conservative_ev_vla_window":(40.2336..=49.1744).contains(&speed) && (20.0..=35.0).contains(&vla),
            "still_air_force_comparison":reference::force_comparison(speed,spin,settings.density_kg_m3).map_err(anyhow::Error::msg)?,"models":models}));
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"source":"fixed_launch_flight_comparison_v2","status":"reference_comparison_not_calibrated",
        "engine_source_sha256":saberbb::domain::seeded_diagnostic::ENGINE,"reference_source":reference::SOURCE_URL,
        "settings":settings,"reference_constants":{"mass_kg":reference::MASS_KG,"radius_m":reference::RADIUS_M,"gravity_ms2":saberbb::domain::util::GRAVITY},"live_dt_sec":LIVE_FLIGHT_DT_SEC,"rows":rows,
        "definitions":["No database, players, RNG, contact selection or saved stats. All models share launch/height/wind/gravity. HLA and spin angle are zero: straight-ahead backspin.",
            "The published2017 force coefficients were fitted to 2016 Statcast trajectories, not individual measured reference endpoints in this file. Mass/circumference use the paper's nominal ball; density=1.194 kg/m3; common g=9.81 m/s2. No claim of calibrated saberbb flight.",
            "live_discrete explicitly selects historical drag; published_drag_discrete selects the new default. Both call the same implementation with explicit environment and stops at first ground in a wide 500m field. Normal gameplay retains its original .90m height, stadium wind, fence/bounce/rolling and .01s scheme.",
            "Other rows use an independent RK4 first-ground solver with interpolated ground event, no fence/bounce/rolling. live_forces_rk4 retains historical forces; published_drag_live_lift controls the new default numerical-method differences. Hybrid rows change only drag or lift law; published_drag_live_lift is now the default force combination; other hybrids remain attribution controls.",
            "published2017 holds world spin fixed; tau30 is optional exponential-decay sensitivity, not a new measurement. Initial-force summaries use the launch speed (still-air scalar), while integrations use relative air velocity when wind is supplied.",
            "The 120m,2m diagnostic marker is a plane y=120m crossing before landing, not a stadium home-run decision. Errors/boundary exits stay explicit, never zero carry. The conservative window is a comparison subset, not a guarantee outside or inside it."]})
        )?
    );
    Ok(())
}
