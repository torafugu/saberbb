use super::swing_attribute_flight::{FrozenFlightAudit, POWER_DELTA, SPEED_DELTA_MS};
use super::{
    Args, Counts, FairBallOutcomes, GameResult, GameSchedule, accumulate, derived_seed,
    fair_launch_audit::HIGH_EV_MIN_MS, simulate_game,
};
use anyhow::{Context, Result, ensure};
use rusqlite::Connection;
use saberbb::domain::{
    random_provider::RealRng,
    resolver::batting_calibration::{
        BAT_CONTROL_CORRECTION_MIDPOINT, BATTING_EYE_TIMING_MIDPOINT, with_calibrated_batting,
    },
    shared::game_state::with_line_drive_attack_angle_trial,
    strategy::batting_strategy::BattingStrategySettings,
};
use saberbb::{
    domain::{
        game_service::{GAME_SWING_SPEED_BONUS_MS, simulate_game_with_pitching},
        resolver::batting_resolver::SwingContactType,
    },
    repositories::game_repository::SqlGameRepository,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Serialize)]
struct UpperTail {
    threshold: f64,
    players_above_threshold: usize,
}

impl UpperTail {
    fn from_values(mut values: Vec<f64>) -> Self {
        values.sort_by(f64::total_cmp);
        // Nearest-rank 90th percentile of unique roster batters.
        let threshold = values[(9 * values.len()).div_ceil(10) - 1];
        let players_above_threshold = values.iter().filter(|&&value| value > threshold).count();
        Self {
            threshold,
            players_above_threshold,
        }
    }

    fn compress(&self, value: f64) -> f64 {
        if value > self.threshold {
            self.threshold + 0.5 * (value - self.threshold)
        } else {
            value
        }
    }
}

#[derive(Clone, Copy)]
enum AbilityAdjustment {
    Current,
    CompressUpperTail,
    Limits { mean_drop: f64, cap: Option<f64> },
}

impl AbilityAdjustment {
    fn apply(self, value: f64, tail: &UpperTail) -> f64 {
        match self {
            Self::Current => value,
            Self::CompressUpperTail => tail.compress(value),
            Self::Limits { mean_drop, cap } => {
                let shifted = value - mean_drop;
                cap.map_or(shifted, |cap| shifted.min(cap))
            }
        }
    }
}

fn ability_distribution(values: impl Iterator<Item = f64>, roster_size: usize) -> Value {
    let (sum, upper) = values.fold((0.0_f64, f64::NEG_INFINITY), |(sum, upper), value| {
        (sum + value / roster_size as f64, upper.max(value))
    });
    json!({"mean": sum, "max": upper})
}

#[derive(Default, Serialize)]
struct BattingAverageDistribution {
    below_200: u64,
    from_200_below_250: u64,
    from_250_through_300: u64,
    above_300_below_400: u64,
    from_400_below_500: u64,
    at_least_500: u64,
}

impl BattingAverageDistribution {
    fn observe(&mut self, hits: u64, ab: u64) {
        // Compare exact batting averages so .250 and .300 are both included.
        let scaled_hits = 1000 * hits;
        match scaled_hits {
            n if n < 200 * ab => self.below_200 += 1,
            n if n < 250 * ab => self.from_200_below_250 += 1,
            n if n <= 300 * ab => self.from_250_through_300 += 1,
            n if n < 400 * ab => self.above_300_below_400 += 1,
            n if n < 500 * ab => self.from_400_below_500 += 1,
            _ => self.at_least_500 += 1,
        }
    }
}

#[derive(Default, Serialize)]
struct PlateAppearanceBand {
    batters: u64,
    without_at_bats: u64,
    batting_average_distribution: BattingAverageDistribution,
}

impl PlateAppearanceBand {
    fn observe(&mut self, hits: u64, ab: u64) {
        self.batters += 1;
        if ab == 0 {
            self.without_at_bats += 1;
        } else {
            self.batting_average_distribution.observe(hits, ab);
        }
    }
}

const PA_BANDS: [&str; 5] = [
    "below_25_pct",
    "25_to_below_50_pct",
    "50_to_below_75_pct",
    "75_to_below_100_pct",
    "at_least_100_pct",
];

fn plate_appearance_band(pa: u64, required_pa: u64) -> usize {
    if pa >= required_pa {
        4
    } else {
        (4 * pa / required_pa) as usize
    }
}

fn rates(counts: &Counts) -> Value {
    let mut summary = counts.json();
    let per_100_pa = |events| (counts.pa != 0).then(|| 100.0 * events as f64 / counts.pa as f64);
    summary["hr_per_100_pa"] = json!(per_100_pa(counts.hr));
    summary["walks_per_100_pa"] = json!(per_100_pa(counts.walks));
    summary["strikeouts_per_100_pa"] = json!(per_100_pa(counts.strikeouts));
    summary
}

#[derive(Default, Serialize)]
struct AttackAngleLaunchBands {
    vla_7_to_under_15: FairBallOutcomes,
    vla_30_to_under_40: FairBallOutcomes,
}

impl AttackAngleLaunchBands {
    fn observe_game(&mut self, game: &GameResult) {
        for batting in &game.player_battings {
            let ev = batting.ball.launch_speed;
            if !ev.is_finite() || ev < HIGH_EV_MIN_MS {
                continue;
            }
            let angle = batting.ball.launch_angle;
            if (7.0..15.0).contains(&angle) {
                self.vla_7_to_under_15.observe(batting.result);
            } else if (30.0..40.0).contains(&angle) {
                self.vla_30_to_under_40.observe(batting.result);
            }
        }
    }
}

pub(super) fn run(conn: &Connection, args: &Args, games: &[GameSchedule]) -> Result<Value> {
    const HOME_RUN_TARGET: u64 = 20;
    const SPEED_SWEEP: [(&str, f64); 4] = [
        ("current", 0.0),
        ("swing_speed_plus_1_ms", SPEED_DELTA_MS),
        ("swing_speed_plus_1_5_ms", 1.5),
        ("swing_speed_plus_2_ms", 2.0),
    ];
    // Each pair is the roster attack-angle increase in degrees and the swing-power increase.
    const ANGLE_POWER_TRIALS: [(&str, f64, f64); 4] = [
        ("live_speed_baseline", 0.0, 0.0),
        ("attack_angle_plus_1_deg", 1.0, 0.0),
        ("swing_power_plus_0_25", 0.0, 0.25),
        ("angle_and_power", 1.0, 0.25),
    ];
    let swing_attributes = args.balance_swing_attributes;
    let angle_power_tuning = args.balance_angle_power_tuning;
    let speed_sweep = args.balance_swing_speed_sweep;
    let pregame = if swing_attributes {
        SqlGameRepository::replay_pregame(conn, games)?
    } else {
        Vec::new()
    };
    let roster: BTreeMap<_, _> = games
        .iter()
        .flat_map(|game| [&game.away_team, &game.home_team])
        .flat_map(|team| {
            team.players
                .iter()
                .map(move |player| (player.info.id, (team.id, player)))
        })
        .collect();
    let skills: Vec<_> = roster
        .values()
        .filter_map(|(_, p)| p.offense_skills.batter)
        .collect();
    ensure!(
        !skills.is_empty(),
        "no roster batters for the batting balance comparison"
    );
    ensure!(
        skills
            .iter()
            .all(|b| b.bat_control.is_finite() && b.batting_eye.is_finite()),
        "non-finite roster batting ability"
    );
    let bat_control = UpperTail::from_values(skills.iter().map(|b| b.bat_control).collect());
    let batting_eye = UpperTail::from_values(skills.iter().map(|b| b.batting_eye).collect());
    let angle_deg = f64::from(args.balance_line_drive_attack_deg.unwrap_or(2));

    let mut arms = Vec::new();
    let mean_drop = args.balance_ability_mean_drop.unwrap_or(0.5);
    let cap = args.balance_ability_cap.unwrap_or(1.0);
    ensure!(
        mean_drop.is_finite() && mean_drop > 0.0 && cap.is_finite(),
        "ability mean drop must be positive and both settings must be finite"
    );
    use AbilityAdjustment::{CompressUpperTail, Current, Limits};
    let mean_drop_with_angle =
        args.balance_ability_limits && args.balance_line_drive_attack_deg.is_some();
    let shifted = Limits {
        mean_drop,
        cap: None,
    };
    let model_calibration = args.balance_model_calibration;
    let attack_angle_sweep = args.balance_attack_angle_sweep;
    let split_attack_angle = args.balance_split_attack_angle;
    // Angle pairs contain the roster change and the additional LineDrive change.
    let trials = if angle_power_tuning {
        ANGLE_POWER_TRIALS
            .iter()
            .map(|&(label, angle, _)| (label, Current, (angle, 0.0), true))
            .collect()
    } else if speed_sweep {
        SPEED_SWEEP
            .iter()
            .map(|&(label, _)| (label, Current, (0.0, 0.0), true))
            .collect()
    } else if swing_attributes {
        vec![
            ("current", Current, (0.0, 0.0), true),
            ("swing_power_plus_1", Current, (0.0, 0.0), true),
            ("swing_speed_plus_1_ms", Current, (0.0, 0.0), true),
        ]
    } else if split_attack_angle {
        vec![
            ("current", Current, (0.0, 0.0), true),
            ("line_drive_plus_6", Current, (0.0, 6.0), true),
            ("roster_plus_3_line_drive_plus_3", Current, (3.0, 3.0), true),
            ("roster_plus_6", Current, (6.0, 0.0), true),
        ]
    } else if attack_angle_sweep {
        vec![
            ("current", Current, (0.0, 0.0), true),
            ("line_drive_plus_3", Current, (0.0, 3.0), true),
            ("line_drive_plus_6", Current, (0.0, 6.0), true),
            ("line_drive_plus_9", Current, (0.0, 9.0), true),
        ]
    } else if model_calibration {
        vec![
            ("current", Current, (0.0, 0.0), false),
            ("generation_angle_only", Current, (2.0, 0.0), false),
            ("model_only", Current, (0.0, 0.0), true),
            ("generation_angle_and_model", Current, (2.0, 0.0), true),
        ]
    } else if mean_drop_with_angle {
        vec![
            ("current", Current, (0.0, 0.0), false),
            ("mean_drop_only", shifted, (0.0, 0.0), false),
            ("angle_only", Current, (0.0, angle_deg), false),
            ("mean_drop_and_angle", shifted, (0.0, angle_deg), false),
        ]
    } else if args.balance_ability_limits {
        vec![
            ("current", Current, (0.0, 0.0), false),
            ("mean_drop_only", shifted, (0.0, 0.0), false),
            (
                "cap_only",
                Limits {
                    mean_drop: 0.0,
                    cap: Some(cap),
                },
                (0.0, 0.0),
                false,
            ),
            (
                "mean_drop_and_cap",
                Limits {
                    mean_drop,
                    cap: Some(cap),
                },
                (0.0, 0.0),
                false,
            ),
        ]
    } else {
        vec![
            ("current", Current, (0.0, 0.0), false),
            ("compression_only", CompressUpperTail, (0.0, 0.0), false),
            ("angle_only", Current, (0.0, angle_deg), false),
            ("combined", CompressUpperTail, (0.0, angle_deg), false),
        ]
    };
    let case_count = if args.balance_current_only {
        1
    } else {
        trials.len()
    };
    for (
        arm_index,
        (label, adjustment, (roster_angle_delta_deg, line_drive_delta_deg), calibrated),
    ) in trials.into_iter().take(case_count).enumerate()
    {
        let (power_delta, speed_delta) = if angle_power_tuning {
            (ANGLE_POWER_TRIALS[arm_index].2, GAME_SWING_SPEED_BONUS_MS)
        } else {
            match (swing_attributes, speed_sweep, arm_index) {
                (_, true, index) => (0.0, SPEED_SWEEP[index].1),
                (true, false, 1) => (POWER_DELTA, 0.0),
                (true, false, 2) => (0.0, SPEED_DELTA_MS),
                _ => (0.0, 0.0),
            }
        };
        let mut pooled = Counts::default();
        let mut reports = Vec::new();
        let (mut seeds_reaching_target, mut pooled_swings, mut pooled_whiffs) = (0, 0_u64, 0_u64);
        for &seed in &args.seeds {
            let mut total = Counts::default();
            let mut launch_bands = attack_angle_sweep.then(AttackAngleLaunchBands::default);
            let mut teams = BTreeMap::new();
            let mut players = BTreeMap::new();
            let mut frozen_flight =
                (swing_attributes && !speed_sweep && !angle_power_tuning && arm_index == 0)
                    .then(FrozenFlightAudit::default);
            let (mut swings, mut whiffs) = (0_u64, 0_u64);
            for (index, original) in games.iter().enumerate() {
                let mut game = original.clone();
                if !matches!(adjustment, Current)
                    || roster_angle_delta_deg != 0.0
                    || power_delta != 0.0
                    || speed_delta != 0.0
                {
                    for player in game
                        .away_team
                        .players
                        .iter_mut()
                        .chain(&mut game.home_team.players)
                    {
                        if let Some(batter) = &mut player.offense_skills.batter {
                            batter.bat_control = adjustment.apply(batter.bat_control, &bat_control);
                            batter.batting_eye = adjustment.apply(batter.batting_eye, &batting_eye);
                            if swing_attributes {
                                batter.swing_power += power_delta;
                                batter.swing_speed += speed_delta;
                            }
                            if roster_angle_delta_deg != 0.0 {
                                batter.attack_angle += roster_angle_delta_deg;
                            }
                        }
                    }
                }
                let run = || {
                    let rng = Box::new(RealRng::from_seed(derived_seed(seed, original.id)));
                    if let Some(history) = pregame.get(index) {
                        return simulate_game_with_pitching(
                            game,
                            rng,
                            &history.pitcher_loads,
                            &history.pitching_stats,
                            Some(&history.position_fatigue),
                            Some(args.max_pitches),
                            false,
                        );
                    }
                    simulate_game(
                        game,
                        rng,
                        BattingStrategySettings::player_driven(),
                        Some(args.max_pitches),
                        false,
                    )
                };
                let run = || with_line_drive_attack_angle_trial(line_drive_delta_deg, run);
                let result = if calibrated {
                    with_calibrated_batting(run)
                } else {
                    run()
                }
                .with_context(|| {
                    format!("batting balance {label}, seed {seed}, game {}", original.id)
                })?;
                if let Some(bands) = &mut launch_bands {
                    bands.observe_game(&result);
                }
                if let Some(audit) = &mut frozen_flight {
                    audit.observe_game(original, &result);
                }
                if swing_attributes {
                    for batting in &result.player_battings {
                        if batting
                            .swing_decision
                            .is_some_and(|decision| decision.swung)
                        {
                            swings += 1;
                            whiffs += u64::from(
                                batting.adjusted_contact_type == SwingContactType::SwungAndMiss,
                            );
                        }
                    }
                }
                accumulate(&mut total, &mut teams, &mut players, original, &result)?;
            }
            let mut pa_bands: [PlateAppearanceBand; PA_BANDS.len()] = Default::default();
            let mut batter_rows = Vec::new();
            let mut max_home_runs = 0;
            let mut batters_with_at_least_10_hr = 0;
            let mut max_qualified_ba: Option<f64> = None;
            for (id, counts) in players.into_iter().filter(|(_, counts)| counts.pa > 0) {
                let (team_id, player) = roster[&id];
                let team_games = teams[&team_id].games;
                let required_pa = (31 * team_games + 5) / 10;
                let band = plate_appearance_band(counts.pa, required_pa);
                let hits = counts.single + counts.double + counts.triple + counts.hr;
                pa_bands[band].observe(hits, counts.ab);
                let ba = (counts.ab > 0).then(|| hits as f64 / counts.ab as f64);
                let qualified = ba.is_some() && counts.pa >= required_pa;
                max_home_runs = max_home_runs.max(counts.hr);
                batters_with_at_least_10_hr += u64::from(counts.hr >= 10);
                if let Some(ba) = ba.filter(|_| qualified) {
                    max_qualified_ba = Some(max_qualified_ba.map_or(ba, |best| best.max(ba)));
                }
                batter_rows.push(json!({
                    "batter_id": id, "team_id": team_id,
                    "first_name": player.info.first_name, "last_name": player.info.last_name,
                    "team_games": team_games, "games_with_batting_record": counts.games,
                    "pa": counts.pa, "ab": counts.ab, "hits": hits, "hr": counts.hr,
                    "ba": ba,
                    "required_pa": required_pa, "pa_shortfall": required_pa.saturating_sub(counts.pa),
                    "qualified": qualified,
                }));
            }
            let qualified = &pa_bands[4];
            let distribution = &qualified.batting_average_distribution;
            let at_least_400 = distribution.from_400_below_500 + distribution.at_least_500;
            let mut report = json!({
                "seed": seed.to_string(), "league": rates(&total),
                "team_games": teams.iter().map(|(&id, c)| (id, c.games)).collect::<BTreeMap<_, _>>(),
                "qualified_batters": qualified.batters - qualified.without_at_bats,
                "qualified_at_least_400": at_least_400,
                "qualified_batting_average_distribution": distribution,
                "batters": batter_rows,
                "batting_average_by_pa": PA_BANDS.into_iter().zip(pa_bands).collect::<BTreeMap<_, _>>(),
            });
            if attack_angle_sweep || swing_attributes {
                report["max_home_runs"] = json!(max_home_runs);
                report["max_qualified_ba"] = json!(max_qualified_ba);
            }
            if let Some(bands) = launch_bands {
                report["hard_fair_launch_bands"] = json!(bands);
            }
            if swing_attributes {
                report["swings"] = json!(swings);
                report["whiffs"] = json!(whiffs);
                report["whiff_rate"] = json!((swings > 0).then(|| whiffs as f64 / swings as f64));
                if let Some(audit) = frozen_flight {
                    report["frozen_flight"] = json!(audit);
                }
            }
            if angle_power_tuning {
                report["batters_with_at_least_10_hr"] = json!(batters_with_at_least_10_hr);
            }
            if speed_sweep || angle_power_tuning {
                seeds_reaching_target += u64::from(max_home_runs >= HOME_RUN_TARGET);
                pooled_swings += swings;
                pooled_whiffs += whiffs;
            }
            reports.push(report);
            eprintln!(
                "Batting balance {label}, seed {seed}: {} games complete",
                games.len()
            );
            pooled.merge(total);
        }
        let mut arm = json!({
            "arm": label, "compress_upper_tail": matches!(adjustment, CompressUpperTail),
            "line_drive_attack_delta_deg": line_drive_delta_deg, "pooled_league": rates(&pooled), "reports": reports,
        });
        if swing_attributes {
            arm["swing_power_delta"] = json!(power_delta);
            arm["swing_speed_delta_ms"] = json!(speed_delta);
        }
        if speed_sweep || angle_power_tuning {
            arm["seeds_reaching_hr_target"] = json!(seeds_reaching_target);
            arm["pooled_whiff_rate"] =
                json!((pooled_swings > 0).then(|| pooled_whiffs as f64 / pooled_swings as f64));
        }
        if args.balance_ability_limits {
            arm["roster_abilities"] = json!({
                "bat_control": ability_distribution(
                    skills.iter().map(|b| adjustment.apply(b.bat_control, &bat_control)), skills.len(),
                ),
                "batting_eye": ability_distribution(
                    skills.iter().map(|b| adjustment.apply(b.batting_eye, &batting_eye)), skills.len(),
                ),
            });
        }
        if model_calibration || split_attack_angle || angle_power_tuning {
            arm["calibrated_batting_model"] = json!(calibrated);
            arm["roster_attack_angle_delta_deg"] = json!(roster_angle_delta_deg);
        }
        arms.push(arm);
    }
    let mut output = json!({
        "definition": "Replay independent regular-season games with the same original rosters, schedules and starting seeds. Later pitches and results can diverge. Qualification is evaluated separately per seed, using each batter's team game count and 3.1 PA per game rounded to the nearest whole PA. Batting averages are unrounded for banding. The batter rows and PA bands include players with at least one PA; PA bands use PA divided by the team qualification threshold. Players with no AB have no batting average. The roster angle delta applies to both batting intents, and the LineDrive delta adds to that intent’s execution angle.",
        "arms": arms,
    });
    if swing_attributes {
        output["swing_attributes"] = json!({
            "game_replay": "Uses the regular-game lineup and pitching policies with frozen pregame workload and pitching statistics from the source database. Trial games do not update that history. Ability deltas apply to cloned rosters and can affect lineup selections.",
            "pitcher_workload_games_with_nonzero_load": pregame.iter().filter(|h| h.pitcher_loads.values().any(|&x| x > 0.0)).count(),
            "position_workload_games_with_nonzero_load": pregame.iter().filter(|h| h.position_fatigue.values().any(|&x| x > 0.0)).count(),
        });
        let settings = &mut output["swing_attributes"];
        if angle_power_tuning {
            settings["home_run_target"] = json!(HOME_RUN_TARGET);
            settings["live_speed_bonus_ms"] = json!(GAME_SWING_SPEED_BONUS_MS);
            settings["angle_power_tuning"] = json!(
                "All arms replay full games with the live swing-speed bonus. Added roster angle and power are measured independently and together."
            );
        } else if speed_sweep {
            settings["home_run_target"] = json!(HOME_RUN_TARGET);
            settings["speed_sweep_deltas_ms"] = json!(SPEED_SWEEP.map(|(_, delta)| delta));
        } else {
            settings["frozen_flight"] = json!(
                "Replays the control arm's original fair contacts with both contact geometry and launch direction and spin fixed. Only exit speed changes. Counts include only contacts whose baseline and both alternatives replayed; home runs here are trajectory outcomes."
            );
            settings["power_delta"] = json!(POWER_DELTA);
            settings["speed_delta_ms"] = json!(SPEED_DELTA_MS);
        }
    } else if attack_angle_sweep {
        output["attack_angle_sweep"] = json!({
            "definition": "All arms use the regular-game batting response coefficients. The launch bands count terminal fair outcomes across all intents, including home runs. Replays use the diagnostic batting totals, which count sacrifice-fly outs as at-bats.",
            "min_exit_speed_ms": HIGH_EV_MIN_MS,
            "split_roster_and_line_drive_offsets": split_attack_angle,
        });
    } else if model_calibration {
        output["model_calibration"] = json!({
            "definition": "The current arm uses the legacy model and saved roster angles. The angle arms add 2 degrees to each input roster angle to approximate shifting the generation distribution, retaining individual differences and applying to both intents. The model arms move the midpoint of the bat-control displacement correction and batting-eye timing response from zero to 0.5.",
            "bat_control_correction_midpoint": BAT_CONTROL_CORRECTION_MIDPOINT,
            "batting_eye_timing_midpoint": BATTING_EYE_TIMING_MIDPOINT,
        });
    } else if mean_drop_with_angle {
        output["mean_drop_with_angle"] = json!({
            "definition": "Subtract the same amount from bat_control and batting_eye on a cloned roster in the mean-drop arms. Compare with the LineDrive execution angle increase alone and with both changes together. The standard lineup and batting logic are run using the trial abilities.",
            "roster_batters": skills.len(), "mean_drop": mean_drop,
            "line_drive_attack_delta_deg": angle_deg,
        });
    } else if args.balance_ability_limits {
        output["ability_limits"] = json!({
            "definition": "Apply the same shift and cap to both bat_control and batting_eye on a cloned roster. In the combined arm subtract the shift first, then cap the result. The standard lineup and batting logic are run using the trial abilities.",
            "roster_batters": skills.len(), "mean_drop": mean_drop, "cap": cap,
        });
    } else {
        output["compression"] = json!({
            "definition": "Find each attribute's nearest-rank 90th percentile among unique roster batters in the selected schedules. Keep values at or below it and halve the excess above it. Use the same thresholds across all arms and seeds.",
            "roster_batters": skills.len(), "retained_excess_fraction": 0.5,
            "bat_control": bat_control, "batting_eye": batting_eye,
        });
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ability_shift_and_angle_can_be_combined_but_cannot_also_set_a_cap() {
        use clap::Parser;
        let mut args = vec![
            "offense_season_check",
            "--season",
            "2026",
            "--batting-balance-comparison",
            "--balance-ability-limits",
            "--balance-ability-mean-drop",
            "0.5",
            "--balance-line-drive-attack-deg",
            "2",
        ];
        let parsed = Args::try_parse_from(&args).unwrap();
        assert!(parsed.balance_ability_limits);
        assert_eq!(parsed.balance_ability_mean_drop, Some(0.5));
        assert_eq!(parsed.balance_line_drive_attack_deg, Some(2));
        args.extend(["--balance-ability-cap", "1.0"]);
        assert!(Args::try_parse_from(&args).is_err());
    }

    #[test]
    fn ability_limits_shift_then_cap_without_raising_lower_values() {
        let tail = UpperTail::from_values(vec![0.0, 1.5]);
        let shifted = AbilityAdjustment::Limits {
            mean_drop: 0.5,
            cap: None,
        };
        let capped = AbilityAdjustment::Limits {
            mean_drop: 0.0,
            cap: Some(1.0),
        };
        let combined = AbilityAdjustment::Limits {
            mean_drop: 0.5,
            cap: Some(1.0),
        };
        assert_eq!(shifted.apply(2.0, &tail), 1.5);
        assert_eq!(capped.apply(2.0, &tail), 1.0);
        assert_eq!(capped.apply(-0.2, &tail), -0.2);
        assert_eq!(combined.apply(2.0, &tail), 1.0);
        assert_eq!(combined.apply(0.0, &tail), -0.5);
        assert_eq!(
            ability_distribution([0.0, 1.0].into_iter(), 2),
            json!({"mean": 0.5, "max": 1.0})
        );
    }

    #[test]
    fn upper_tail_compression_preserves_the_threshold_and_values_below_it() {
        let tail = UpperTail::from_values((1..=10).map(f64::from).collect());
        assert_eq!(tail.threshold, 9.0);
        assert_eq!(tail.players_above_threshold, 1);
        assert_eq!(tail.compress(1.0), 1.0);
        assert_eq!(tail.compress(9.0), 9.0);
        assert_eq!(tail.compress(10.0), 9.5);
    }

    #[test]
    fn plate_appearance_bands_preserve_qualification_boundaries() {
        for (pa, band) in [
            (1, 0),
            (109, 0),
            (110, 1),
            (219, 2),
            (328, 3),
            (436, 3),
            (437, 4),
            (438, 4),
        ] {
            assert_eq!(plate_appearance_band(pa, 437), band);
        }
        let mut band = PlateAppearanceBand::default();
        band.observe(0, 0);
        band.observe(1, 4);
        assert_eq!(band.batters, 2);
        assert_eq!(band.without_at_bats, 1);
        assert_eq!(band.batting_average_distribution.from_250_through_300, 1);
    }

    #[test]
    fn batting_average_bands_include_the_requested_boundaries() {
        let mut bands = BattingAverageDistribution::default();
        for hits in [199, 200, 249, 250, 300, 301, 399, 400, 499, 500] {
            bands.observe(hits, 1000);
        }
        assert_eq!(
            json!(bands),
            json!({
                "below_200": 1, "from_200_below_250": 2, "from_250_through_300": 2,
                "above_300_below_400": 2, "from_400_below_500": 2, "at_least_500": 1,
            })
        );
    }
}
