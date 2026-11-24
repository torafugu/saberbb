use super::swing_attribute_flight::{
    CappedContactFlightAudit, CappedFlightAdjustment, EXIT_SPEED_REDUCTIONS, FrozenFlightAudit,
    LAUNCH_ANGLE_SHIFTS, POWER_DELTA, SPEED_DELTA_MS,
};
use super::{
    Args, Counts, FairBallOutcomes, GameResult, GameSchedule, accumulate, bip_audit::band, derived_seed,
    fair_launch_audit::HIGH_EV_MIN_MS, simulate_game,
};
use anyhow::{Context, Result, ensure};
use rusqlite::Connection;
use saberbb::domain::{
    random_provider::RealRng,
    resolver::batting_calibration::{
        BAT_CONTROL_CORRECTION_MIDPOINT, BATTING_EYE_TIMING_MIDPOINT, with_calibrated_batting,
    },
    shared::{
        game_state::with_line_drive_attack_angle_trial, game_stats::PlayerGameBatting,
        player::Position, team::MAX_LINEUP_PLAYERS,
    },
    strategy::batting_strategy::BattingStrategySettings,
};
use saberbb::{
    domain::{
        game_service::{
            GAME_BATTING_EYE_REDUCTION, GAME_SWING_SPEED_BONUS_MS,
            simulate_game_with_pitching_and_lineup_source,
        },
        resolver::batting_resolver::{
            FAST_SWING_ERROR_FULL_MS, FAST_SWING_ERROR_START_MS, SwingContactType,
            with_collision_power_trial, with_fast_swing_error_trial,
        },
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
        self.retain_excess(value, 0.5)
    }

    fn retain_excess(&self, value: f64, fraction: f64) -> f64 {
        if value > self.threshold && fraction != 1.0 {
            self.threshold + fraction * (value - self.threshold)
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

const CONTACT_EV_EDGES_MS: &[f64] = &[40.0, 45.0, 47.5, 50.0, 52.5, 55.0];
const CONTACT_VLA_EDGES_DEG: &[f64] = &[15.0, 20.0, 25.0, 30.0, 35.0, 40.0];

#[derive(Default)]
struct BatterContactAudit {
    counts: Counts,
    swings: u64,
    whiffs: u64,
    fair: FairBallOutcomes,
    hard_fair_25_to_30: u64,
    adjacent_25_to_35: FairBallOutcomes,
    hard_25_to_35: FairBallOutcomes,
    fair_by_ev_and_vla: BTreeMap<String, BTreeMap<String, FairBallOutcomes>>,
    fair_contacts: Vec<Value>,
    exit_speed_replay: Option<CappedContactFlightAudit>,
    launch_angle_replay: Option<CappedContactFlightAudit>,
}

impl BatterContactAudit {
    fn observe(&mut self, game: &GameSchedule, batting: &PlayerGameBatting) {
        self.counts.batting(batting.result);
        if batting.swing_decision.is_some_and(|decision| decision.swung) {
            self.swings += 1;
            self.whiffs += u64::from(batting.adjusted_contact_type == SwingContactType::SwungAndMiss);
        }
        let prior_fair_plays = self.fair.plays;
        self.fair.observe(batting.result);
        if self.fair.plays == prior_fair_plays {
            return;
        }
        if batting.ball.launch_speed >= 50.0 && (25.0..30.0).contains(&batting.ball.launch_angle) {
            self.hard_fair_25_to_30 += 1;
        }
        if (25.0..35.0).contains(&batting.ball.launch_angle) {
            if batting.ball.launch_speed >= 50.0 {
                self.hard_25_to_35.observe(batting.result);
            } else if batting.ball.launch_speed >= 47.5 {
                self.adjacent_25_to_35.observe(batting.result);
            }
        }
        for replay in self
            .exit_speed_replay
            .iter_mut()
            .chain(&mut self.launch_angle_replay)
        {
            replay.observe(&game.stadium, batting);
        }
        self.fair_by_ev_and_vla
            .entry(band(batting.ball.launch_speed, CONTACT_EV_EDGES_MS))
            .or_default()
            .entry(band(batting.ball.launch_angle, CONTACT_VLA_EDGES_DEG))
            .or_default()
            .observe(batting.result);
        self.fair_contacts.push(json!({
            "game_id": game.id,
            "count_seq": batting.count_seq,
            "stadium_id": game.stadium.id,
            "pitcher_id": batting.pitcher_id,
            "result": batting.result,
            "contact_type": batting.adjusted_contact_type,
            "exit_speed_ms": batting.ball.launch_speed,
            "launch_angle_deg": batting.ball.launch_angle,
            "spray_angle_deg": batting.final_hla_deg,
            "spin_rpm": batting.ball.spin_rate,
            "timing_error_ms": batting.timing_error * 1_000.0,
            "signed_length_offset_m": batting.signed_length_offset_m,
            "signed_thickness_offset_m": batting.signed_thickness_offset_m,
        }));
    }

    fn into_json(self, fixed_lineups: bool, include_hard_launch_bands: bool) -> Value {
        let mut output = json!({
            "pa": self.counts.pa,
            "ab": self.counts.ab,
            "strikeouts": self.counts.strikeouts,
            "walks": self.counts.walks,
            "hit_by_pitch": self.counts.hit_by_pitch,
            "hr_per_pa": ratio(self.counts.hr, self.counts.pa),
            "swings": self.swings,
            "whiffs": self.whiffs,
            "fair_outcomes": self.fair,
            "fair_per_pa": ratio(self.fair.plays, self.counts.pa),
            "hr_per_fair": ratio(self.fair.home_run, self.fair.plays),
            "fair_outcomes_by_ev_and_vla": self.fair_by_ev_and_vla,
            "fair_contacts": self.fair_contacts,
        });
        if let Some(replay) = self.exit_speed_replay {
            output["exit_speed_flight_replay"] = json!(replay);
        }
        if let Some(replay) = self.launch_angle_replay {
            output["launch_angle_flight_replay"] = json!(replay);
        }
        if fixed_lineups {
            output["ev_at_least_50_vla_25_to_30"] = json!(self.hard_fair_25_to_30);
            output["ev_at_least_50_vla_25_to_30_per_fair"] =
                json!(ratio(self.hard_fair_25_to_30, self.fair.plays));
        }
        if include_hard_launch_bands {
            output["vla_25_to_under_35"] = json!({
                "ev_47_5_to_under_50": self.adjacent_25_to_35,
                "ev_at_least_50": self.hard_25_to_35,
                "ev_47_5_to_under_50_per_pa": ratio(self.adjacent_25_to_35.plays, self.counts.pa),
                "ev_at_least_50_per_pa": ratio(self.hard_25_to_35.plays, self.counts.pa),
            });
        }
        output
    }
}

fn ratio(numerator: u64, denominator: u64) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
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
    let live_batting_abilities = args.balance_live_batting_abilities;
    let angle_power_tuning = args.balance_angle_power_tuning;
    let speed_sweep = args.balance_swing_speed_sweep;
    let speed_tail_sweep = args.balance_swing_speed_upper_tail;
    let speed_cap_sweep = args.balance_swing_speed_cap_sweep;
    let capped_tail_comparison = args.balance_capped_swing_tail_comparison;
    let capped_power_tail_sweep = args.balance_capped_power_tail_sweep;
    let fixed_lineups = args.balance_capped_power_fixed_lineups;
    let fast_swing_sweep = args.balance_fast_swing_error_sweep;
    let median_power_replay = args.balance_contact_median_power_replay;
    let collision_power_sweep = args.balance_collision_power_sweep;
    let capped_zero_bonus = fast_swing_sweep || median_power_replay || collision_power_sweep;
    if capped_zero_bonus {
        ensure!(
            std::env::var("SABERBB_SWING_EXECUTION").as_deref() == Ok("mixture"),
            "the zero-bonus capped comparison requires SABERBB_SWING_EXECUTION=mixture"
        );
    }
    let speed_distribution_sweep =
        speed_tail_sweep || speed_cap_sweep || capped_tail_comparison || capped_zero_bonus;
    const COMMON_CAP_MS: f64 = 35.5;
    const SPEED_TAIL_START_MS: f64 = 35.0;
    const SPEED_EXCESS_RETAINED: f64 = 0.5;
    const POWER_EXCESS_RETAINED: f64 = 0.75;
    let speed_caps_ms = [None, Some(36.0), Some(35.5), Some(35.0)];
    let speed_tail_fractions = [1.0, 0.75, 0.50, 0.25];
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
    let mut contact_batters = BTreeMap::new();
    for &id in &args.balance_contact_batter_ids {
        let (_, player) = roster
            .get(&id)
            .with_context(|| format!("contact audit batter {id} is not on a scheduled roster"))?;
        let batter = player
            .offense_skills
            .batter
            .with_context(|| format!("contact audit player {id} has no batting abilities"))?;
        ensure!(
            contact_batters.insert(id, batter).is_none(),
            "duplicate contact audit batter {id}"
        );
    }
    if args.balance_contact_swap_batting_attributes {
        ensure!(
            contact_batters.len() == 2,
            "contact attribute swap requires exactly two contact audit batter IDs"
        );
        ensure!(
            contact_batters.values().all(|batter| batter.swing_power.is_finite()),
            "contact attribute swap requires finite swing power for both batters"
        );
    }
    let flight_reference = args
        .balance_contact_flight_reference_id
        .map(|id| {
            roster.get(&id)
                .and_then(|(_, player)| player.offense_skills.batter)
                .with_context(|| format!("flight reference batter {id} has no batting abilities on the scheduled rosters"))
        })
        .transpose()?;
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
    let speed_tail = if speed_distribution_sweep {
        ensure!(
            skills.iter().all(|b| b.swing_speed.is_finite() && b.swing_speed > 0.0),
            "invalid roster swing speed"
        );
        Some(UpperTail::from_values(skills.iter().map(|b| b.swing_speed).collect()))
    } else {
        None
    };
    let bat_control = UpperTail::from_values(skills.iter().map(|b| b.bat_control).collect());
    let batting_eye = UpperTail::from_values(skills.iter().map(|b| b.batting_eye).collect());
    let power_tail = if capped_tail_comparison {
        ensure!(
            skills.iter().all(|b| b.swing_power.is_finite()),
            "invalid roster swing power"
        );
        Some(UpperTail::from_values(skills.iter().map(|b| b.swing_power).collect()))
    } else {
        None
    };
    let median_power = if median_power_replay || collision_power_sweep {
        let mut powers: Vec<_> = skills.iter().map(|batter| batter.swing_power).collect();
        ensure!(
            powers.iter().all(|power| power.is_finite()),
            "invalid roster swing power"
        );
        powers.sort_by(f64::total_cmp);
        let n = powers.len();
        Some(powers[(n - 1) / 2].midpoint(powers[n / 2]))
    } else {
        None
    };
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
    // Each pair specifies the amount to subtract from bat control and batting eye.
    let batting_ability_trials: &[(&str, f64, f64)] = if median_power_replay {
        &[("fast_swing_control", 0.0, GAME_BATTING_EYE_REDUCTION)]
    } else if collision_power_sweep {
        &[
            ("fast_swing_control", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("collision_power_half", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("collision_power_median", 0.0, GAME_BATTING_EYE_REDUCTION),
        ]
    } else if fast_swing_sweep {
        &[
            ("fast_swing_control", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("fast_swing_error_125", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("fast_swing_error_150", 0.0, GAME_BATTING_EYE_REDUCTION),
        ]
    } else if capped_power_tail_sweep {
        &[
            ("speed_cap_35_5", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("capped_power_tail", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("capped_power_tail_keep_50", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("capped_power_tail_keep_25", 0.0, GAME_BATTING_EYE_REDUCTION),
        ]
    } else if capped_tail_comparison {
        &[
            ("speed_cap_35_5", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("capped_speed_tail", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("capped_power_tail", 0.0, GAME_BATTING_EYE_REDUCTION),
        ]
    } else if speed_cap_sweep {
        &[
            ("live_speed_baseline", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("speed_cap_36_0", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("speed_cap_35_5", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("speed_cap_35_0", 0.0, GAME_BATTING_EYE_REDUCTION),
        ]
    } else if speed_tail_sweep {
        &[
            ("live_speed_baseline", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("speed_tail_keep_75", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("speed_tail_keep_50", 0.0, GAME_BATTING_EYE_REDUCTION),
            ("speed_tail_keep_25", 0.0, GAME_BATTING_EYE_REDUCTION),
        ]
    } else if args.balance_batting_eye_sweep {
        &[
            ("live_speed_baseline", 0.0, 0.0),
            ("batting_eye_minus_0_15", 0.0, 0.15),
            ("batting_eye_minus_0_25", 0.0, 0.25),
        ]
    } else {
        &[
            ("live_speed_baseline", 0.0, 0.0),
            ("lower_bat_control", mean_drop, 0.0),
            ("lower_batting_eye", 0.0, mean_drop),
            ("lower_control_and_eye", mean_drop, mean_drop),
        ]
    };
    let trials = if live_batting_abilities {
        batting_ability_trials
            .iter()
            .map(|&(label, _, _)| (label, Current, (0.0, 0.0), true))
            .collect()
    } else if angle_power_tuning {
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
    let mut baseline_hr_leaders = BTreeMap::<u64, Vec<i64>>::new();
    let mut baseline_qualified_below_speed_tail = BTreeMap::<u64, Vec<i64>>::new();
    let mut control_lineups = BTreeMap::<(u64, u32), Vec<(i64, Position, u8)>>::new();
    for (
        arm_index,
        (label, adjustment, (roster_angle_delta_deg, line_drive_delta_deg), calibrated),
    ) in trials.into_iter().take(case_count).enumerate()
    {
        let (control_drop, eye_drop, control_adjustment, eye_adjustment) = if live_batting_abilities
        {
            let (_, control_drop, eye_drop) = batting_ability_trials[arm_index];
            let reduce = |mean_drop| {
                if mean_drop == 0.0 {
                    Current
                } else {
                    Limits {
                        mean_drop,
                        cap: None,
                    }
                }
            };
            (
                control_drop,
                eye_drop,
                reduce(control_drop),
                reduce(eye_drop),
            )
        } else {
            (0.0, 0.0, adjustment, adjustment)
        };
        let (power_delta, speed_delta) = if capped_zero_bonus {
            (0.0, 0.0)
        } else if live_batting_abilities {
            (0.0, GAME_SWING_SPEED_BONUS_MS)
        } else if angle_power_tuning {
            (ANGLE_POWER_TRIALS[arm_index].2, GAME_SWING_SPEED_BONUS_MS)
        } else {
            match (swing_attributes, speed_sweep, arm_index) {
                (_, true, index) => (0.0, SPEED_SWEEP[index].1),
                (true, false, 1) => (POWER_DELTA, 0.0),
                (true, false, 2) => (0.0, SPEED_DELTA_MS),
                _ => (0.0, 0.0),
            }
        };
        let retained_speed_excess = speed_tail_sweep.then(|| speed_tail_fractions[arm_index]);
        let swing_speed_cap_ms = if capped_tail_comparison || capped_zero_bonus {
            Some(COMMON_CAP_MS)
        } else {
            speed_cap_sweep.then(|| speed_caps_ms[arm_index]).flatten()
        };
        let replay_exit_speed =
            args.balance_contact_exit_speed_replay && swing_speed_cap_ms == Some(35.5);
        let replay_launch_angle =
            args.balance_contact_launch_angle_replay && swing_speed_cap_ms == Some(35.5);
        let swap_contact_attributes =
            args.balance_contact_swap_batting_attributes && swing_speed_cap_ms == Some(35.5);
        let compress_capped_speed = label == "capped_speed_tail";
        let fast_swing_max_scale = match label {
            "fast_swing_error_125" => 1.25,
            "fast_swing_error_150" => 1.50,
            _ => 1.0,
        };
        let retained_collision_power = match label {
            "collision_power_half" => 0.5,
            "collision_power_median" => 0.0,
            _ => 1.0,
        };
        let retained_power_excess = match label {
            "capped_power_tail" => Some(POWER_EXCESS_RETAINED),
            "capped_power_tail_keep_50" => Some(0.50),
            "capped_power_tail_keep_25" => Some(0.25),
            _ => None,
        };
        let effective_speed = |speed: f64| {
            let speed = speed_tail.as_ref().zip(retained_speed_excess)
                .map_or(speed, |(tail, fraction)| tail.retain_excess(speed, fraction));
            let live_speed = speed + speed_delta;
            let capped = swing_speed_cap_ms.map_or(live_speed, |cap| live_speed.min(cap));
            if compress_capped_speed && capped > SPEED_TAIL_START_MS {
                SPEED_TAIL_START_MS + (capped - SPEED_TAIL_START_MS) * SPEED_EXCESS_RETAINED
            } else {
                capped
            }
        };
        let effective_power = |power: f64| {
            let power = power + power_delta;
            power_tail
                .as_ref()
                .zip(retained_power_excess)
                .map_or(power, |(tail, fraction)| tail.retain_excess(power, fraction))
        };
        let mut pooled = Counts::default();
        let mut reports = Vec::new();
        let (mut seeds_reaching_target, mut pooled_swings, mut pooled_whiffs) = (0, 0_u64, 0_u64);
        for &seed in &args.seeds {
            let mut total = Counts::default();
            let mut launch_bands = attack_angle_sweep.then(AttackAngleLaunchBands::default);
            let mut teams = BTreeMap::new();
            let mut players = BTreeMap::new();
            let mut frozen_flight = (swing_attributes
                && !speed_sweep
                && !angle_power_tuning
                && !live_batting_abilities
                && arm_index == 0)
                .then(FrozenFlightAudit::default);
            let mut contact_audit = BTreeMap::<i64, BatterContactAudit>::new();
            if speed_cap_sweep || fixed_lineups || capped_zero_bonus || label == "batting_eye_minus_0_25" {
                contact_audit.extend(contact_batters.keys().map(|&id| {
                    (
                        id,
                        BatterContactAudit {
                            exit_speed_replay: replay_exit_speed.then(|| {
                                CappedContactFlightAudit::new(CappedFlightAdjustment::ExitSpeed)
                            }),
                            launch_angle_replay: replay_launch_angle.then(|| {
                                CappedContactFlightAudit::new(CappedFlightAdjustment::LaunchAngle)
                            }),
                            ..BatterContactAudit::default()
                        },
                    )
                }));
            }
            let mut paired_flight = BTreeMap::new();
            if let Some(reference) = flight_reference {
                for &id in contact_audit.keys() {
                    let batter = contact_batters[&id];
                    paired_flight.insert(id, FrozenFlightAudit::for_batter(
                        id,
                        reference.swing_speed - batter.swing_speed,
                        reference.swing_power - batter.swing_power,
                    ));
                }
            }
            if swap_contact_attributes {
                for ((&id, batter), (&peer_id, peer)) in
                    contact_batters.iter().zip(contact_batters.iter().rev())
                {
                    paired_flight.insert(
                        id,
                        FrozenFlightAudit::for_capped_swap(
                            id,
                            peer_id,
                            effective_speed(peer.swing_speed) - effective_speed(batter.swing_speed),
                            peer.swing_power - batter.swing_power,
                        ),
                    );
                }
            }
            if median_power_replay && let Some(median) = median_power {
                for &id in contact_audit.keys() {
                    paired_flight.insert(id, FrozenFlightAudit::for_median_power(id, median));
                }
            }
            let (mut swings, mut whiffs) = (0_u64, 0_u64);
            for (index, original) in games.iter().enumerate() {
                let apply_abilities = |game: &mut GameSchedule, use_control_power: bool| {
                    if !matches!(control_adjustment, Current)
                        || !matches!(eye_adjustment, Current)
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
                                batter.bat_control =
                                    control_adjustment.apply(batter.bat_control, &bat_control);
                                batter.batting_eye =
                                    eye_adjustment.apply(batter.batting_eye, &batting_eye);
                                if swing_attributes {
                                    if !use_control_power {
                                        batter.swing_power = effective_power(batter.swing_power);
                                    }
                                    batter.swing_speed = effective_speed(batter.swing_speed);
                                }
                                if roster_angle_delta_deg != 0.0 {
                                    batter.attack_angle += roster_angle_delta_deg;
                                }
                            }
                        }
                    }
                };
                let mut game = original.clone();
                apply_abilities(&mut game, false);
                let lineup_source = (fixed_lineups && arm_index > 0).then(|| {
                    let mut control = original.clone();
                    apply_abilities(&mut control, true);
                    control
                });
                let run = || {
                    let rng = Box::new(RealRng::from_seed(derived_seed(seed, original.id)));
                    if let Some(history) = pregame.get(index) {
                        return simulate_game_with_pitching_and_lineup_source(
                            game,
                            rng,
                            &history.pitcher_loads,
                            &history.pitching_stats,
                            Some(&history.position_fatigue),
                            Some(args.max_pitches),
                            false,
                            lineup_source.as_ref(),
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
                let run = || {
                    with_fast_swing_error_trial(fast_swing_max_scale, || {
                        with_line_drive_attack_angle_trial(line_drive_delta_deg, run)
                    })
                };
                let run = || {
                    if collision_power_sweep && let Some(median) = median_power {
                        with_collision_power_trial(median, retained_collision_power, run)
                    } else {
                        run()
                    }
                };
                let result = if calibrated {
                    with_calibrated_batting(run)
                } else {
                    run()
                }
                .with_context(|| {
                    format!("batting balance {label}, seed {seed}, game {}", original.id)
                })?;
                if fixed_lineups {
                    let opening: Vec<_> = result
                        .player_entries
                        .iter()
                        .take(2 * MAX_LINEUP_PLAYERS)
                        .map(|entry| (entry.player_id, entry.position, entry.batting_order))
                        .collect();
                    ensure!(
                        opening.len() == 2 * MAX_LINEUP_PLAYERS,
                        "incomplete opening lineups for game {}",
                        original.id
                    );
                    let key = (seed, original.id);
                    if arm_index == 0 {
                        control_lineups.insert(key, opening);
                    } else {
                        ensure!(
                            control_lineups.get(&key) == Some(&opening),
                            "opening lineup differs from control in {label}, seed {seed}, game {}",
                            original.id
                        );
                    }
                }
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
                for audit in paired_flight.values_mut() {
                    audit.observe_game(original, &result);
                }
                if !contact_audit.is_empty() {
                    for batting in &result.player_battings {
                        if let Some(audit) = contact_audit.get_mut(&batting.batter_id) {
                            audit.observe(original, batting);
                        }
                    }
                }
                accumulate(&mut total, &mut teams, &mut players, original, &result)?;
            }
            let baseline_hr_group = live_batting_abilities.then(|| {
                let ids = baseline_hr_leaders.entry(seed).or_insert_with(|| {
                    let mut leaders: Vec<_> =
                        players.iter().filter(|(_, counts)| counts.hr > 0).collect();
                    leaders
                        .sort_unstable_by_key(|&(&id, counts)| (std::cmp::Reverse(counts.hr), id));
                    leaders.into_iter().take(10).map(|(&id, _)| id).collect()
                });
                let (pa, hr) = ids
                    .iter()
                    .filter_map(|id| players.get(id))
                    .fold((0, 0), |(pa, hr), counts| (pa + counts.pa, hr + counts.hr));
                json!({
                    "batter_ids": ids,
                    "pa": pa,
                    "hr": hr,
                    "hr_per_100_pa": (pa > 0).then(|| 100.0 * hr as f64 / pa as f64),
                })
            });
            let speed_tail_cohort = speed_tail.as_ref().map(|tail| {
                let ids = baseline_qualified_below_speed_tail.entry(seed).or_insert_with(|| {
                    players.iter().filter_map(|(&id, counts)| {
                        let (team_id, player) = roster[&id];
                        let required_pa = (31 * teams[&team_id].games + 5) / 10;
                        let speed = player.offense_skills.batter?.swing_speed;
                        (speed <= tail.threshold && counts.ab > 0 && counts.pa >= required_pa)
                            .then_some(id)
                    }).collect()
                });
                let mut home_run_rates = Vec::new();
                for id in ids.iter() {
                    if let Some(counts) = players.get(id).filter(|counts| counts.pa > 0) {
                        home_run_rates.push(100.0 * counts.hr as f64 / counts.pa as f64);
                    }
                }
                home_run_rates.sort_by(f64::total_cmp);
                let n = home_run_rates.len();
                let median = (n > 0).then(|| {
                    (home_run_rates[(n - 1) / 2] + home_run_rates[n / 2]) / 2.0
                });
                json!({
                    "batter_ids": ids,
                    "batters_with_pa": n,
                    "median_hr_per_100_pa": median,
                })
            });
            let mut pa_bands: [PlateAppearanceBand; PA_BANDS.len()] = Default::default();
            let mut batter_rows = Vec::new();
            let mut max_home_runs = 0;
            let mut capped_batters_with_pa = 0;
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
                let mut row = json!({
                    "batter_id": id, "team_id": team_id,
                    "first_name": player.info.first_name, "last_name": player.info.last_name,
                    "team_games": team_games, "games_with_batting_record": counts.games,
                    "pa": counts.pa, "ab": counts.ab, "hits": hits, "hr": counts.hr,
                    "ba": ba,
                    "required_pa": required_pa, "pa_shortfall": required_pa.saturating_sub(counts.pa),
                    "qualified": qualified,
                });
                if speed_distribution_sweep {
                    if let Some(batter) = player.offense_skills.batter {
                        let baseline = batter.swing_speed + speed_delta;
                        let effective = effective_speed(batter.swing_speed);
                        row["baseline_swing_speed_ms"] = json!(baseline);
                        row["effective_swing_speed_ms"] = json!(effective);
                        if capped_tail_comparison {
                            row["baseline_swing_power"] = json!(batter.swing_power);
                            row["effective_swing_power"] = json!(effective_power(batter.swing_power));
                        }
                        capped_batters_with_pa += usize::from(
                            swing_speed_cap_ms.is_some_and(|cap| baseline > cap)
                        );
                    }
                }
                batter_rows.push(row);
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
            if !contact_audit.is_empty() {
                report["batter_contact_audit"] = json!(
                    contact_audit
                        .into_iter()
                        .map(|(id, audit)| (id, audit.into_json(fixed_lineups, capped_zero_bonus)))
                        .collect::<BTreeMap<_, _>>()
                );
            }
            if !paired_flight.is_empty() {
                report["batter_contact_paired_flight"] = json!(paired_flight);
            }
            if fixed_lineups {
                report["verified_opening_lineup_games"] = json!(games.len());
            }
            if speed_cap_sweep || capped_tail_comparison {
                report["capped_batters_with_pa"] = json!(capped_batters_with_pa);
            }
            if let Some(cohort) = speed_tail_cohort {
                report["baseline_qualified_at_or_below_speed_p90"] = cohort;
            }
            if let Some(group) = baseline_hr_group {
                report["baseline_hr_leaders"] = group;
            }
            if angle_power_tuning || live_batting_abilities {
                report["batters_with_at_least_10_hr"] = json!(batters_with_at_least_10_hr);
            }
            if speed_sweep || angle_power_tuning || live_batting_abilities {
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
        if speed_sweep || angle_power_tuning || live_batting_abilities {
            arm["seeds_reaching_hr_target"] = json!(seeds_reaching_target);
            arm["pooled_whiff_rate"] =
                json!((pooled_swings > 0).then(|| pooled_whiffs as f64 / pooled_swings as f64));
        }
        if live_batting_abilities {
            arm["bat_control_delta"] = json!(0.0 - control_drop);
            arm["batting_eye_delta"] = json!(0.0 - eye_drop);
        }
        if let Some(fraction) = retained_speed_excess {
            arm["retained_speed_excess_fraction"] = json!(fraction);
        }
        if speed_cap_sweep || capped_tail_comparison {
            let affected = skills.iter().filter(|batter| {
                swing_speed_cap_ms.is_some_and(|cap| batter.swing_speed + speed_delta > cap)
            }).count();
            arm["effective_swing_speed_cap_ms"] = json!(swing_speed_cap_ms);
            arm["capped_roster_batters"] = json!(affected);
        }
        if speed_distribution_sweep {
            arm["effective_roster_swing_speed_ms"] = ability_distribution(
                skills.iter().map(|b| effective_speed(b.swing_speed)), skills.len(),
            );
        }
        if capped_tail_comparison {
            arm["effective_roster_swing_power"] = ability_distribution(
                skills.iter().map(|b| effective_power(b.swing_power)), skills.len(),
            );
            arm["speed_tail_affected_roster_batters"] = json!(
                if compress_capped_speed {
                    skills.iter().filter(|b| b.swing_speed + speed_delta > SPEED_TAIL_START_MS).count()
                } else { 0 }
            );
            arm["power_tail_affected_roster_batters"] = json!(
                power_tail.as_ref().filter(|_| retained_power_excess.is_some())
                    .map_or(0, |tail| tail.players_above_threshold)
            );
        }
        if capped_power_tail_sweep {
            arm["power_excess_retained_fraction"] = json!(retained_power_excess.unwrap_or(1.0));
        }
        if fast_swing_sweep {
            arm["fast_swing_max_error_scale"] = json!(fast_swing_max_scale);
        }
        if capped_zero_bonus {
            arm["effective_swing_speed_cap_ms"] = json!(swing_speed_cap_ms);
        }
        if collision_power_sweep {
            arm["collision_power_retained_fraction"] = json!(retained_collision_power);
            arm["pooled_league"]["doubles_per_100_pa"] =
                json!(ratio(pooled.double, pooled.pa).map(|value| 100.0 * value));
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
    if collision_power_sweep && let Some(median) = median_power {
        output["collision_power_sweep"] = json!({
            "roster_swing_power_median": median,
            "roster_batters": skills.len(),
            "swing_execution_model": "mixture",
            "swing_speed_bonus_ms": 0.0,
            "swing_speed_cap_ms": COMMON_CAP_MS,
            "definition": "Retain the arm fraction of the difference between each batter's collision coefficient and the coefficient at roster-median swing power, at the same contact depth. Roster power and its other uses such as lineup scoring are preserved. Play complete games in every arm.",
        });
    } else if let Some(median) = median_power {
        output["contact_median_power_replay"] = json!({
            "roster_swing_power_median": median,
            "roster_batters": skills.len(),
            "swing_execution_model": "mixture",
            "swing_speed_bonus_ms": 0.0,
            "swing_speed_cap_ms": COMMON_CAP_MS,
            "definition": "Use the median of the unique scheduled roster batters' swing power. Replace only the power input to the collision calculation, holding pitch speed, bat speed, timing depth, contact offsets, launch angles, spin and park fixed. Compare only contacts whose baseline reconstructs the recorded exit speed, flight and home-run status. Group by original exit speed and launch angle.",
        });
    } else if fast_swing_sweep {
        output["fast_swing_error_sweep"] = json!({
            "swing_execution_model": "mixture",
            "swing_speed_bonus_ms": 0.0,
            "swing_speed_cap_ms": COMMON_CAP_MS,
            "error_scale_start_ms": FAST_SWING_ERROR_START_MS,
            "error_scale_full_ms": FAST_SWING_ERROR_FULL_MS,
            "definition": "Multiply the existing stochastic timing and spatial execution errors by a speed-dependent factor. It is 1 at or below the starting speed and rises linearly to the arm maximum at the full speed. Pitch prediction and the collision and flight formulas keep their original inputs and rules. Virtual game paths may diverge after different outcomes.",
        });
    } else if capped_power_tail_sweep && let (Some(power), Some(speed)) = (&power_tail, &speed_tail) {
        output["capped_power_tail_sweep"] = json!({
            "base_speed_cap_ms": COMMON_CAP_MS,
            "roster_swing_power_p90": power.threshold,
            "uncapped_roster_swing_speed_p90_ms": speed.threshold,
            "roster_batters": skills.len(),
            "swing_execution_model": std::env::var("SABERBB_SWING_EXECUTION")
                .unwrap_or_else(|_| "legacy".into()),
            "definition": "Keep the same effective swing-speed cap of 35.5 m/s and the live batting-eye reduction in every arm. Use one nearest-rank swing-power P90 from the unique scheduled roster batters for all arms and seeds. Keep values at or below P90 and multiply the excess above it by each arm's retained fraction.",
        });
        if fixed_lineups {
            output["capped_power_tail_sweep"]["opening_lineups"] = json!(
                "Select both opening lineups and batting orders with control abilities under the same pregame workload and seed. Verify player ID, position and batting order against the control for every game. Apply trial power to the active batters. Substitutions and pitching changes use the live policies."
            );
        }
    } else if let (Some(power), Some(speed)) = (&power_tail, &speed_tail) {
        output["capped_swing_tail_comparison"] = json!({
            "base_speed_cap_ms": COMMON_CAP_MS,
            "speed_tail_start_ms": SPEED_TAIL_START_MS,
            "speed_excess_retained_fraction": SPEED_EXCESS_RETAINED,
            "roster_swing_power_p90": power.threshold,
            "uncapped_roster_swing_speed_p90_ms": speed.threshold,
            "power_excess_retained_fraction": POWER_EXCESS_RETAINED,
            "roster_batters": skills.len(),
            "swing_execution_model": std::env::var("SABERBB_SWING_EXECUTION")
                .unwrap_or_else(|_| "legacy".into()),
            "definition": "Start from the same 35.5 m/s cap after the live speed bonus in every arm. The speed arm retains half of capped speed above 35.0 m/s. The power arm retains three quarters of swing power above the nearest-rank P90 of unique scheduled roster batters. Only one attribute changes per trial. The baseline-qualified speed cohort uses uncapped roster-speed P90.",
        });
    } else if let Some(tail) = speed_tail {
        let (key, definition) = if speed_cap_sweep {
            ("swing_speed_cap_sweep", "Apply each absolute cap after the regular-game speed bonus. Count unique roster batters whose live speed exceeds the cap, and report how many of those batters had at least one PA per replay. The median follows the same baseline-qualified batters at or below the uncapped speed P90, including batters affected by a cap.")
        } else {
            ("swing_speed_upper_tail", "Use the nearest-rank P90 of unique scheduled roster batters. Retain the stated fraction of each batter's speed above P90, then add the regular-game speed bonus. The median home-run rate uses the same baseline-qualified batters at or below speed P90 for each seed, excluding anyone with no trial PA.")
        };
        output[key] = json!({
            "roster_p90_ms": tail.threshold,
            "live_p90_ms": tail.threshold + GAME_SWING_SPEED_BONUS_MS,
            "roster_batters": skills.len(),
            "players_above_threshold": tail.players_above_threshold,
            "swing_execution_model": std::env::var("SABERBB_SWING_EXECUTION")
                .unwrap_or_else(|_| "legacy".into()),
            "definition": definition,
        });
    }
    if !contact_batters.is_empty() {
        output["contact_audit"] = json!({
            "swing_execution_model": std::env::var("SABERBB_SWING_EXECUTION")
                .unwrap_or_else(|_| "legacy".into()),
            "roster_batting_abilities": contact_batters,
            "exit_speed_edges_ms": CONTACT_EV_EDGES_MS,
            "vertical_launch_angle_edges_deg": CONTACT_VLA_EDGES_DEG,
            "definition": "Fair contacts use the replay's counted fair plate-appearance outcomes, including home runs. Joint bands are keyed by exit speed first and vertical launch angle second, with inclusive lower and exclusive upper bounds. Roster abilities are reported before the arm's deltas and speed cap. Each fair contact is keyed by game ID and count sequence within its seed.",
        });
    }
    if args.balance_contact_exit_speed_replay {
        output["contact_exit_speed_replay"] = json!({
            "arm": "speed_cap_35_5",
            "exit_speed_reductions_ms": EXIT_SPEED_REDUCTIONS.into_iter().collect::<BTreeMap<_, _>>(),
            "definition": "Replay counted fair balls with original exit speed 47.5 to under 52.5 m/s and vertical launch angle 20 to under 35 degrees. Report transitions by original band, holding direction, spin rate and axis, stadium, and flight settings fixed. Require the reconstructed baseline to match the recorded flight and recorded home-run status; all reductions use the same accepted contacts. Count fence-clearing home runs from the flight model.",
        });
    }
    if args.balance_contact_launch_angle_replay {
        output["contact_launch_angle_replay"] = json!({
            "arm": "speed_cap_35_5",
            "vertical_launch_angle_shifts_deg": LAUNCH_ANGLE_SHIFTS.into_iter().collect::<BTreeMap<_, _>>(),
            "definition": "Replay counted fair balls with original exit speed 47.5 to under 52.5 m/s and vertical launch angle 20 to under 35 degrees. Report transitions by original band, holding exit speed, spray angle, spin rate and axis, stadium, and flight settings fixed. Require the reconstructed baseline to match the recorded flight and recorded home-run status; all angle shifts use the same accepted contacts. Count fence-clearing home runs from the flight model.",
        });
    }
    if let Some(reference) = flight_reference {
        output["contact_flight_reference"] = json!({
            "batter_id": args.balance_contact_flight_reference_id,
            "roster_swing_speed_ms": reference.swing_speed,
            "roster_swing_power": reference.swing_power,
            "definition": "Apply roster speed or power differences to each recorded swing. Pitch speed, contact offsets, timing depth, launch angles, spin and park are fixed. Count home runs only on pairs whose baseline reproduces the recorded exit speed and flight. This replay does not estimate effects on swing decisions or making contact.",
        });
    }
    if args.balance_contact_swap_batting_attributes {
        output["contact_batting_attribute_swap"] = json!({
            "arm": "speed_cap_35_5",
            "definition": "Swap the two batters' effective roster swing speeds after the live bonus and cap, their swing power, and both independently on each recorded contact. Hold pitch speed, contact offsets, timing depth, launch angles, spin and park fixed. Require the baseline to reproduce measured exit speed, flight and home-run status. Report exit-speed deltas and home-run transitions on matched contacts. Group the 25 to under 30 degree contacts by absolute signed offsets split at 5 cm (length) and 2 cm (thickness).",
        });
    }
    if swing_attributes {
        output["swing_attributes"] = json!({
            "game_replay": "Uses the regular-game lineup and pitching policies with frozen pregame workload and pitching statistics from the source database. Trial games do not update that history. Ability deltas apply to cloned rosters and can affect lineup selections.",
            "pitcher_workload_games_with_nonzero_load": pregame.iter().filter(|h| h.pitcher_loads.values().any(|&x| x > 0.0)).count(),
            "position_workload_games_with_nonzero_load": pregame.iter().filter(|h| h.position_fatigue.values().any(|&x| x > 0.0)).count(),
        });
        let settings = &mut output["swing_attributes"];
        if live_batting_abilities {
            settings["home_run_target"] = json!(HOME_RUN_TARGET);
            settings["live_speed_bonus_ms"] = json!(if capped_zero_bonus { 0.0 } else { GAME_SWING_SPEED_BONUS_MS });
            if args.balance_batting_eye_sweep {
                settings["batting_eye_sweep_deltas"] = json!(
                    batting_ability_trials
                        .iter()
                        .map(|&(_, _, drop)| 0.0 - drop)
                        .collect::<Vec<_>>()
                );
            } else if !capped_tail_comparison && !capped_zero_bonus {
                settings["batting_ability_drop"] = json!(mean_drop);
            }
            settings["baseline_hr_leaders"] = json!(
                "Per seed, fix the baseline's first 10 batters with at least one home run in descending HR order, breaking ties by player ID. Compare this same group's PA and HR/100 PA in every arm."
            );
        } else if angle_power_tuning {
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
