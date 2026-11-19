//! Replay the same simulated swings with nearby vertical contact offsets.
use super::{
    bip_audit::{band, is_bip},
    vertical_contact_replay::Inputs,
};
use anyhow::{Context, Result, ensure};
use saberbb::domain::{
    random_provider::RealRng,
    resolver::batting_resolver::{
        CONTACT_TIMING_POSITION_SCALE, SwingContactType, calculate_dynamic_attack_angle,
        contact_geometry_is_miss, project_contact_offsets, sample_face_angle_terms, wrap_hla_deg,
    },
    seeded_diagnostic::derived_seed,
    shared::{
        ball::{BattedBall, OutboundResult, PitchedBall},
        game::{BattingResult, GameResult, GameSchedule},
        game_stats::PlayerGameBatting,
        player::BatterInfo,
        stadium::Stadium,
    },
    util::calculate_polar_distance,
};
use serde::Serialize;
use std::collections::BTreeMap;

const SHIFTS_MM: [i32; 5] = [-4, -2, 0, 2, 4];
const HIGH_EV_MS: f64 = 45.0;
const TARGET_ANGLE: std::ops::Range<f64> = 7.0..10.0;

fn fair_trajectory(ball: &BattedBall) -> bool {
    match ball.outbound_result {
        OutboundResult::Foul => false,
        OutboundResult::InField => !ball.is_foul(),
        _ => true,
    }
}

fn fast_low(ball: &BattedBall) -> bool {
    fair_trajectory(ball)
        && ball.launch_speed >= HIGH_EV_MS
        && TARGET_ANGLE.contains(&ball.launch_angle)
}

fn flight_band(ball: Option<&BattedBall>) -> String {
    match ball {
        None => "geometric_miss".into(),
        Some(ball) if !fair_trajectory(ball) => "foul".into(),
        Some(ball) if ball.launch_speed < HIGH_EV_MS => "below_45_ms".into(),
        Some(ball) => band(
            ball.launch_angle,
            &[-10.0, 0.0, 5.0, 7.0, 10.0, 15.0, 20.0, 25.0, 35.0, 50.0],
        ),
    }
}

#[derive(Default, Serialize)]
struct Scenario {
    geometric_misses: u64,
    lost_contacts: u64,
    recovered_contacts: u64,
    fair_trajectories: u64,
    high_ev_fair_trajectories: u64,
    high_ev_fair_by_angle: BTreeMap<String, u64>,
    fast_low_balls: u64,
    original_fast_low_destinations: BTreeMap<String, u64>,
    newly_fast_low_origins: BTreeMap<String, u64>,
    original_home_runs_retained: u64,
    new_home_run_trajectories: u64,
}

impl Scenario {
    fn observe(
        &mut self,
        baseline: Option<&BattedBall>,
        trial: Option<&BattedBall>,
        original_hr: bool,
    ) {
        self.geometric_misses += u64::from(trial.is_none());
        self.lost_contacts += u64::from(baseline.is_some() && trial.is_none());
        self.recovered_contacts += u64::from(baseline.is_none() && trial.is_some());

        let became_hr = trial.is_some_and(|ball| ball.outbound_result == OutboundResult::HomeRun);
        self.original_home_runs_retained += u64::from(original_hr && became_hr);
        self.new_home_run_trajectories += u64::from(!original_hr && became_hr);
        if let Some(ball) = trial.filter(|ball| fair_trajectory(ball)) {
            self.fair_trajectories += 1;
            if ball.launch_speed >= HIGH_EV_MS {
                self.high_ev_fair_trajectories += 1;
                *self
                    .high_ev_fair_by_angle
                    .entry(flight_band(Some(ball)))
                    .or_default() += 1;
            }
        }
        let trial_fast_low = trial.is_some_and(fast_low);
        self.fast_low_balls += u64::from(trial_fast_low);
        if baseline.is_some_and(fast_low) {
            *self
                .original_fast_low_destinations
                .entry(flight_band(trial))
                .or_default() += 1;
        } else if trial_fast_low {
            *self
                .newly_fast_low_origins
                .entry(flight_band(baseline))
                .or_default() += 1;
        }
    }
}

#[derive(Serialize)]
pub(super) struct Sweep {
    source_swings: u64,
    source_misses: u64,
    source_home_runs: u64,
    source_official_fast_low_balls: u64,
    comparable_swings: u64,
    comparable_misses: u64,
    comparable_home_runs: u64,
    comparable_official_fast_low_balls: u64,
    exclusions: BTreeMap<String, u64>,
    exclusions_by_shift_mm: BTreeMap<i32, u64>,
    by_shift_mm: BTreeMap<i32, Scenario>,
}

impl Default for Sweep {
    fn default() -> Self {
        Self {
            source_swings: 0,
            source_misses: 0,
            source_home_runs: 0,
            source_official_fast_low_balls: 0,
            comparable_swings: 0,
            comparable_misses: 0,
            comparable_home_runs: 0,
            comparable_official_fast_low_balls: 0,
            exclusions: BTreeMap::new(),
            exclusions_by_shift_mm: BTreeMap::new(),
            by_shift_mm: SHIFTS_MM
                .into_iter()
                .map(|shift| (shift, Scenario::default()))
                .collect(),
        }
    }
}

impl Sweep {
    pub(super) fn observe_game(
        &mut self,
        schedule: &GameSchedule,
        game: &GameResult,
        game_seed: u64,
        geometry_scale: f64,
    ) {
        let batters: BTreeMap<_, _> = schedule
            .away_team
            .players
            .iter()
            .chain(&schedule.home_team.players)
            .filter_map(|p| Some((p.info.id, p.offense_skills.batter.as_ref()?)))
            .collect();
        let pitches: BTreeMap<_, _> = game
            .player_pitchings
            .iter()
            .map(|p| ((p.count_seq, p.pitcher_id), &p.ball))
            .collect();

        for batting in &game.player_battings {
            if batting.adjusted_contact_type == SwingContactType::Take {
                continue;
            }
            let missed = batting.adjusted_contact_type == SwingContactType::SwungAndMiss;
            let original_hr = batting.result == BattingResult::HomeRun;
            let official_fast_low = is_bip(batting.result)
                && batting.ball.launch_speed >= HIGH_EV_MS
                && TARGET_ANGLE.contains(&batting.ball.launch_angle);
            self.source_swings += 1;
            self.source_misses += u64::from(missed);
            self.source_home_runs += u64::from(original_hr);
            self.source_official_fast_low_balls += u64::from(official_fast_low);

            let baseline_inputs = || {
                let batter = batters.get(&batting.batter_id).context("missing_batter")?;
                let pitch = pitches
                    .get(&(batting.count_seq, batting.pitcher_id))
                    .context("missing_pitch")?;
                reconstruct(
                    batting,
                    batter,
                    pitch,
                    missed,
                    game_seed,
                    geometry_scale,
                    &schedule.stadium,
                )
            };
            let inputs = match baseline_inputs() {
                Ok(inputs) => inputs,
                Err(err) => {
                    *self.exclusions.entry(err.to_string()).or_default() += 1;
                    continue;
                }
            };
            let baseline = (!missed).then_some(&batting.ball);
            let mut flights = Vec::with_capacity(SHIFTS_MM.len());
            for shift in SHIFTS_MM {
                let flight = if shift == 0 {
                    Ok(baseline.copied())
                } else {
                    inputs.flight(inputs.z + f64::from(shift) / 1000.0, &schedule.stadium)
                };
                match flight {
                    Ok(flight) => flights.push((shift, flight)),
                    Err(_) => *self.exclusions_by_shift_mm.entry(shift).or_default() += 1,
                }
            }
            if flights.len() != SHIFTS_MM.len() {
                *self
                    .exclusions
                    .entry("trial_trajectory_failed".into())
                    .or_default() += 1;
                continue;
            }

            self.comparable_swings += 1;
            self.comparable_misses += u64::from(missed);
            self.comparable_home_runs += u64::from(original_hr);
            self.comparable_official_fast_low_balls += u64::from(official_fast_low);
            for (shift, flight) in flights {
                self.by_shift_mm.get_mut(&shift).unwrap().observe(
                    baseline,
                    flight.as_ref(),
                    original_hr,
                );
            }
        }
    }
}

fn required(value: Option<f64>) -> Result<f64> {
    value
        .filter(|v| v.is_finite())
        .context("missing_or_nonfinite_contact_input")
}

fn reconstruct(
    batting: &PlayerGameBatting,
    batter: &BatterInfo,
    pitch: &PitchedBall,
    missed: bool,
    game_seed: u64,
    geometry_scale: f64,
    stadium: &Stadium,
) -> Result<Inputs> {
    let x = required(batting.contact_offset_x_m)?;
    let z = required(batting.contact_offset_z_m)?;
    let tilt = required(batting.actual_bat_angle_deg)?;
    let depth = if geometry_scale == CONTACT_TIMING_POSITION_SCALE {
        required(batting.timing_impact_x_m)?
    } else {
        batter.swing_speed
            * required(batting.adapted_timing_offset_sec)?
            * CONTACT_TIMING_POSITION_SCALE
    };
    let (length, thickness) = project_contact_offsets(x, z, tilt);
    ensure!(
        (length.abs() - batting.length_offset_m).abs() < 1e-7
            && (thickness.abs() - required(batting.thickness_offset_m)?).abs() < 1e-7,
        "baseline_geometry_mismatch"
    );
    ensure!(
        contact_geometry_is_miss(length, thickness) == missed,
        "baseline_contact_mismatch"
    );

    let bat_speed = if missed {
        batter.swing_speed
    } else {
        batting.bat_speed
    };
    ensure!(
        [
            bat_speed,
            pitch.speed,
            pitch.spin_rate,
            pitch.spin_angle,
            batter.swing_power,
            batter.attack_angle,
            depth
        ]
        .into_iter()
        .all(f64::is_finite)
            && bat_speed > 0.0
            && pitch.speed > 0.0
            && pitch.spin_rate >= 0.0,
        "invalid_swing_inputs"
    );

    let (handedness, face_deg) = if missed {
        let side = batter.batting_side.handedness_sign();
        let mut rng = RealRng::from_seed(derived_seed(game_seed, u32::from(batting.count_seq)));
        let (approach, pull, timing) = sample_face_angle_terms(
            &mut rng,
            side,
            bat_speed,
            required(Some(batting.timing_error))?,
        );
        (side, (approach + pull + timing).to_degrees() * 0.85)
    } else {
        ensure!(
            batting.hla_model_version == Some(2),
            "unsupported_hla_model"
        );
        let side = required(batting.hla_handedness_sign)?;
        let face_deg =
            (batting.approach_variation_deg + batting.pull_bias_deg + batting.timing_angle_error)
                * 0.85;
        ensure!(
            side.abs() == 1.0 && face_deg.is_finite(),
            "invalid_launch_direction_inputs"
        );
        (side, face_deg)
    };
    let inputs = Inputs {
        x,
        z,
        tilt,
        depth,
        pitch_speed: pitch.speed,
        pitch_spin: pitch.spin_rate,
        pitch_axis: pitch.spin_angle,
        bat_speed,
        power: batter.swing_power,
        handedness,
        face_deg,
        attack: calculate_dynamic_attack_angle(batter.attack_angle, tilt),
    };
    if !missed {
        let replayed = inputs
            .flight(z, stadium)?
            .context("baseline_contact_mismatch")?;
        let live = &batting.ball;
        ensure!(
            (replayed.launch_speed - live.launch_speed).abs() < 1e-7
                && (replayed.launch_angle - live.launch_angle).abs() < 1e-7
                && wrap_hla_deg(inputs.launch_direction(thickness) - batting.final_hla_deg).abs()
                    < 1e-7
                && (replayed.spin_rate - live.spin_rate).abs() < 1e-7
                && wrap_hla_deg(replayed.spin_angle - live.spin_angle).abs() < 1e-7
                && replayed.outbound_result == live.outbound_result
                && (replayed.total_time - live.total_time).abs() < 1e-6
                && calculate_polar_distance(&replayed.final_position, &live.final_position) < 1e-5,
            "baseline_trajectory_mismatch"
        );
    }
    Ok(inputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_counts_angle_entries_and_distinguishes_why_balls_leave() {
        let fast = BattedBall {
            launch_speed: 46.0,
            launch_angle: 8.0,
            ..BattedBall::default()
        };
        let shifted = BattedBall {
            launch_angle: 6.0,
            ..fast
        };
        let slowed = BattedBall {
            launch_speed: 44.0,
            ..fast
        };
        let home_run = BattedBall {
            launch_angle: 30.0,
            outbound_result: OutboundResult::HomeRun,
            ..fast
        };

        let mut shifted_summary = Scenario::default();
        for trial in [Some(&shifted), Some(&slowed), None] {
            shifted_summary.observe(Some(&fast), trial, false);
        }
        shifted_summary.observe(None, Some(&fast), false);
        shifted_summary.observe(Some(&home_run), Some(&home_run), true);
        assert_eq!(shifted_summary.fast_low_balls, 1);
        assert_eq!(shifted_summary.original_fast_low_destinations["[5,7)"], 1);
        assert_eq!(
            shifted_summary.original_fast_low_destinations["below_45_ms"],
            1
        );
        assert_eq!(
            shifted_summary.original_fast_low_destinations["geometric_miss"],
            1
        );
        assert_eq!(shifted_summary.newly_fast_low_origins["geometric_miss"], 1);
        assert_eq!(shifted_summary.lost_contacts, 1);
        assert_eq!(shifted_summary.recovered_contacts, 1);
        assert_eq!(shifted_summary.original_home_runs_retained, 1);
    }
}
