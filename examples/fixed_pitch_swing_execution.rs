//! Compare execution models on a known center fastball with a fixed center aim.
use anyhow::Result;
use clap::Parser;
use saberbb::domain::{
    random_provider::RealRng,
    resolver::{
        batting_resolver::{
            SwingContactType, SwingExecutionModel, calculate_batted_ball_with_metrics,
            calculate_swing_execution_error, evaluate_swing_contact, with_swing_execution_model,
        },
        pitching_resolver::PitchDisplacement,
    },
    shared::{
        ball::{BallLocation, OutboundResult, PitchedBall},
        player::{BatterInfo, BatterType, PitchType, RL, ZoneAptitude},
        stadium::Stadium,
    },
    strategy::pitching_strategy::TargetZone,
    util::Vector3D,
};
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value_t = 10_000, value_parser = clap::value_parser!(u32).range(1..))]
    swings: u32,
    #[arg(long, default_value_t = 42)]
    seed: u64,
}

fn quantiles(mut values: Vec<f64>) -> Option<[f64; 3]> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some([0.50, 0.90, 0.99].map(|p| values[(p * (values.len() - 1) as f64).round() as usize]))
}

fn main() -> Result<()> {
    let args = Args::parse();
    let center = BallLocation { x: 0.0, y: 0.0 };
    let pitch = PitchedBall {
        pitch_type: PitchType::FourSeamFastball,
        speed: 36.0,
        spin_rate: 2_000.0,
        spin_angle: 0.0,
        spin_efficiency: 0.95,
        release_point: Vector3D {
            x: 1.6,
            y: 16.74,
            z: 1.7,
        },
        flight_time: 0.46,
        aim_zone: TargetZone::Center,
        aim_location: center,
        actual_location: center,
    };
    let stadium = Stadium::default();
    let displacement = PitchDisplacement::default();
    let mut results = Vec::new();
    for bat_control in [0.0, 1.0, 2.0] {
        let batter = BatterInfo {
            batting_side: RL::Right,
            batter_type: BatterType::ClassicAnalyst,
            zone_aptitude: ZoneAptitude::Balanced,
            hot_zone_scale: 0.0,
            batting_eye: 0.0,
            swing_speed: 33.0,
            swing_power: 1.0,
            attack_angle: 10.0,
            bat_control,
            consistency: 0.0,
        };
        for (name, model) in [
            ("legacy", SwingExecutionModel::Legacy),
            ("gaussian", SwingExecutionModel::Gaussian),
            ("mixture", SwingExecutionModel::Mixture),
        ] {
            let row = with_swing_execution_model(model, || -> Result<_> {
                let mut execution_rng = RealRng::from_seed(args.seed);
                let mut outcomes = BTreeMap::<&str, u32>::new();
                let mut contact_types = BTreeMap::<&str, u32>::new();
                let (mut timing_ms, mut length_cm, mut thickness_cm) =
                    (Vec::new(), Vec::new(), Vec::new());
                let (mut launch_speed, mut launch_angle) = (Vec::new(), Vec::new());
                for index in 0..args.swings {
                    // Use the same outcome seed on each swing even if another arm missed.
                    let mut outcome_rng =
                        RealRng::from_seed((args.seed ^ (1 << 63)).wrapping_add(u64::from(index)));
                    let error =
                        calculate_swing_execution_error(&mut execution_rng, &batter, &center);
                    let contact =
                        evaluate_swing_contact(&mut outcome_rng, &batter, &displacement, &error);
                    timing_ms.push(1_000.0 * contact.timing_offset_sec.abs());
                    length_cm.push(100.0 * contact.length_offset_m);
                    thickness_cm.push(100.0 * contact.thickness_offset_m);
                    let kind = match contact.adjusted_contact_type {
                        SwingContactType::SolidContact => "solid",
                        SwingContactType::WeakContact => "weak",
                        SwingContactType::MarginalContact => "marginal",
                        SwingContactType::SwungAndMiss => "miss",
                        SwingContactType::Take => unreachable!("fixed-pitch trial forces a swing"),
                    };
                    *contact_types.entry(kind).or_default() += 1;
                    let outcome = if contact.adjusted_contact_type == SwingContactType::SwungAndMiss
                    {
                        "miss"
                    } else {
                        let (ball, _) = calculate_batted_ball_with_metrics(
                            &mut outcome_rng,
                            &batter,
                            pitch,
                            &contact,
                            &stadium,
                        )?;
                        launch_speed.push(ball.launch_speed);
                        launch_angle.push(ball.launch_angle);
                        if matches!(ball.outbound_result, OutboundResult::Foul) {
                            "foul"
                        } else {
                            "fair"
                        }
                    };
                    *outcomes.entry(outcome).or_default() += 1;
                }
                Ok(json!({
                    "model": name,
                    "bat_control": bat_control,
                    "outcomes": outcomes,
                    "contact_types": contact_types,
                    "absolute_timing_ms": quantiles(timing_ms),
                    "length_cm": quantiles(length_cm),
                    "thickness_cm": quantiles(thickness_cm),
                    "launch_speed_ms": quantiles(launch_speed),
                    "launch_angle_deg": quantiles(launch_angle),
                }))
            })?;
            results.push(row);
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "seed": args.seed,
            "swings_per_row": args.swings,
            "pitch_speed_ms": pitch.speed,
            "quantiles": [0.50, 0.90, 0.99],
            "results": results,
        }))?
    );
    Ok(())
}
