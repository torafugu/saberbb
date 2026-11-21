use super::{Args, Counts, GameSchedule, accumulate, derived_seed, simulate_game};
use anyhow::{Context, Result, ensure};
use saberbb::domain::{
    random_provider::RealRng,
    shared::{
        ball::OutboundResult,
        game::{BattingResult, GameResult},
        game_state::{
            DoubleFenceObservation, IntentAimOffsets, IntentAimPlay, IntentAimSwing,
            PitchBatTimingTrial, SwingReplay, with_contact_approach_audit,
            with_contact_first_speed_audit, with_contact_length_audit, with_intent_aim_audit,
            with_intent_aim_trial, with_pitch_bat_timing_audit,
        },
    },
    strategy::batting_strategy::{BattedBallIntent, BattingStrategySettings},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[path = "intent_aim_infield.rs"]
mod intent_aim_infield;
use intent_aim_infield::InfieldCounts;

#[path = "contact_approach_quality.rs"]
mod contact_approach_quality;
use contact_approach_quality::ContactApproachQuality;

#[path = "two_strike_approach.rs"]
mod two_strike_approach;
use two_strike_approach::TwoStrikeApproach;

fn arms(args: &Args) -> Vec<(&'static str, IntentAimOffsets)> {
    let baseline = IntentAimOffsets::default();
    let alternative = IntentAimOffsets {
        contact_first_mm: i32::from(args.contact_first_aim_mm.unwrap_or(8)),
        line_drive_mm: i32::from(args.line_drive_aim_mm.unwrap_or(32)),
    };
    let mut arms = vec![("baseline", baseline)];
    if baseline.contact_first_mm != alternative.contact_first_mm {
        arms.push((
            "contact_first_only",
            IntentAimOffsets {
                contact_first_mm: alternative.contact_first_mm,
                ..baseline
            },
        ));
    }
    if baseline.line_drive_mm != alternative.line_drive_mm {
        arms.push((
            "line_drive_only",
            IntentAimOffsets {
                line_drive_mm: alternative.line_drive_mm,
                ..baseline
            },
        ));
    }
    if arms.len() == 3 {
        arms.push(("both", alternative));
    }
    arms
}

const FAIR_ANGLE_BANDS: [&str; 5] = ["below_5", "5_to_10", "10_to_20", "20_to_40", "at_least_40"];

fn fair_angle_band(angle: f64) -> &'static str {
    FAIR_ANGLE_BANDS[if angle < 5.0 {
        0
    } else if angle < 10.0 {
        1
    } else if angle < 20.0 {
        2
    } else if angle < 40.0 {
        3
    } else {
        4
    }]
}

fn target_single(play: SwingReplay) -> bool {
    play.outcome == Some(BattingResult::Single)
        && play
            .flight
            .is_some_and(|(_, _, angle)| (5.0..10.0).contains(&angle))
}

const HARD_LOW: &str = "hard_fair_0_to_10_deg";
const SWING_BANDS: &[&str] = &[
    "whiff",
    "foul",
    HARD_LOW,
    "hard_fair_10_to_20_deg",
    "hard_fair_20_to_30_deg",
    "hard_fair_30_to_40_deg",
    "hard_fair_at_least_40_deg",
    "other_fair",
];

fn swing_band(play: SwingReplay) -> &'static str {
    let Some((outbound, ev, angle)) = play.flight else {
        return "whiff";
    };
    if outbound == OutboundResult::Foul || play.outcome == Some(BattingResult::Foul) {
        return "foul";
    }
    if ev < 45.0 || angle < 0.0 {
        return "other_fair";
    }
    if angle < 10.0 {
        HARD_LOW
    } else if angle < 20.0 {
        "hard_fair_10_to_20_deg"
    } else if angle < 30.0 {
        "hard_fair_20_to_30_deg"
    } else if angle < 40.0 {
        "hard_fair_30_to_40_deg"
    } else {
        "hard_fair_at_least_40_deg"
    }
}

#[derive(Default, serde::Serialize)]
struct DoubleFenceCounts {
    doubles: u64,
    launch_angle_deg: BTreeMap<&'static str, u64>,
    airborne_wall_shortfall_m: BTreeMap<&'static str, u64>,
    projected_first_bounce_fence_gap_m: BTreeMap<&'static str, u64>,
    pickup_fence_gap_m: BTreeMap<&'static str, u64>,
}

impl DoubleFenceCounts {
    fn observe(&mut self, observation: DoubleFenceObservation, angle: f64) {
        self.doubles += 1;
        *self
            .launch_angle_deg
            .entry(fair_angle_band(angle))
            .or_default() += 1;
        if let Some(shortfall) = observation.airborne_wall_shortfall_m {
            let band = if shortfall < 0.25 {
                "below_0_25"
            } else if shortfall < 0.5 {
                "0_25_to_0_5"
            } else if shortfall < 1.0 {
                "0_5_to_1"
            } else {
                "at_least_1"
            };
            *self.airborne_wall_shortfall_m.entry(band).or_default() += 1;
        }
        for (gap, histogram) in [
            (
                observation.first_bounce_fence_gap_m,
                &mut self.projected_first_bounce_fence_gap_m,
            ),
            (observation.pickup_fence_gap_m, &mut self.pickup_fence_gap_m),
        ] {
            if let Some(gap) = gap {
                let band = if gap < 0.0 {
                    "beyond_fence"
                } else if gap < 5.0 {
                    "0_to_5"
                } else if gap < 10.0 {
                    "5_to_10"
                } else if gap < 20.0 {
                    "10_to_20"
                } else if gap < 40.0 {
                    "20_to_40"
                } else {
                    "at_least_40"
                };
                *histogram.entry(band).or_default() += 1;
            }
        }
    }
}

fn outcome_stage(play: SwingReplay) -> &'static str {
    match play.flight {
        None => "whiff",
        Some((OutboundResult::Foul, _, _)) => "foul",
        Some(_) => match play.outcome {
            Some(BattingResult::Foul) => "foul",
            Some(
                BattingResult::Single
                | BattingResult::Double
                | BattingResult::Triple
                | BattingResult::HomeRun,
            ) => "fair_hit",
            Some(_) => "fair_non_hit",
            None => "fair_unresolved",
        },
    }
}

#[derive(Default)]
struct SwingCounts {
    swings: u64,
    contact_types: BTreeMap<String, u64>,
    outcome_stages: BTreeMap<&'static str, u64>,
    bands: BTreeMap<&'static str, u64>,
    transitions: BTreeMap<&'static str, BTreeMap<&'static str, u64>>,
    low_single_transitions: BTreeMap<String, u64>,
    target_single_destinations: BTreeMap<String, BTreeMap<&'static str, u64>>,
    fair_launch_angles: BTreeMap<&'static str, u64>,
    hard_low_singles: u64,
    singles_5_to_10_deg: u64,
    doubles: u64,
    double_fence: BTreeMap<&'static str, DoubleFenceCounts>,
    triples: u64,
    singles: u64,
    home_runs: u64,
    no_batting_result: u64,
    infield: InfieldCounts,
    baseline_fair_retention_by_angle: BTreeMap<&'static str, BTreeMap<u8, u64>>,
    hard_fair_5_to_10_deg: u64,
    retention_launches: u64,
    retention_at_least_80_pct: u64,
    retention_at_least_95_pct: u64,
    outfield_direct_outs: u64,
}

impl SwingCounts {
    fn observe(&mut self, baseline: IntentAimPlay, play: IntentAimPlay) {
        self.infield.observe(baseline, play);
        let (baseline, play) = (baseline.batting, play.batting);
        let destination = match play.flight {
            None => "no_launch",
            Some((OutboundResult::Foul, _, _)) => "foul_flight",
            Some((_, speed, angle)) => {
                let angle_band = fair_angle_band(angle);
                *self.fair_launch_angles.entry(angle_band).or_default() += 1;
                self.hard_fair_5_to_10_deg +=
                    u64::from(speed >= 45.0 && (5.0..10.0).contains(&angle));
                angle_band
            }
        };
        if target_single(baseline) {
            let result = play
                .outcome
                .as_ref()
                .map_or("NoBattingResult", AsRef::as_ref);
            *self
                .target_single_destinations
                .entry(result.to_owned())
                .or_default()
                .entry(destination)
                .or_default() += 1;
        }
        if let Some((outbound, _, angle)) = baseline.flight
            && outbound != OutboundResult::Foul
            && let Some(retention) = play.exit_speed_retention
        {
            let bin = ((retention * 20.0).floor() as u8).min(19) * 5;
            *self
                .baseline_fair_retention_by_angle
                .entry(fair_angle_band(angle))
                .or_default()
                .entry(bin)
                .or_default() += 1;
        }
        let source = swing_band(baseline);
        let band = swing_band(play);
        self.swings += 1;
        *self
            .contact_types
            .entry(play.contact_type.as_ref().to_owned())
            .or_default() += 1;
        *self.outcome_stages.entry(outcome_stage(play)).or_default() += 1;
        *self.bands.entry(band).or_default() += 1;
        *self
            .transitions
            .entry(source)
            .or_default()
            .entry(band)
            .or_default() += 1;
        self.singles += u64::from(play.outcome == Some(BattingResult::Single));
        self.singles_5_to_10_deg += u64::from(target_single(play));
        if let Some(retention) = play.exit_speed_retention {
            self.retention_launches += 1;
            self.retention_at_least_80_pct += u64::from(retention >= 0.80);
            self.retention_at_least_95_pct += u64::from(retention >= 0.95);
        }
        self.outfield_direct_outs += u64::from(play.outfield_direct_out);
        self.doubles += u64::from(play.outcome == Some(BattingResult::Double));
        if let (Some(observation), Some((_, _, angle))) = (play.double_fence, play.flight) {
            self.double_fence
                .entry(observation.category)
                .or_default()
                .observe(observation, angle);
        }
        self.triples += u64::from(play.outcome == Some(BattingResult::Triple));
        self.home_runs += u64::from(play.outcome == Some(BattingResult::HomeRun));
        self.hard_low_singles +=
            u64::from(band == HARD_LOW && play.outcome == Some(BattingResult::Single));
        self.no_batting_result += u64::from(play.outcome.is_none());
        if source == HARD_LOW && baseline.outcome == Some(BattingResult::Single) {
            let result = play
                .outcome
                .as_ref()
                .map_or("NoBattingResult", AsRef::as_ref);
            *self
                .low_single_transitions
                .entry(format!("{band}:{result}"))
                .or_default() += 1;
        }
    }

    fn report(&self) -> Value {
        let rate = |count| (self.swings > 0).then(|| count as f64 / self.swings as f64);
        let bands: BTreeMap<_, _> = SWING_BANDS
            .iter()
            .map(|&band| {
                let n = self.bands.get(band).copied().unwrap_or_default();
                (band, json!({"swings":n,"per_swing":rate(n)}))
            })
            .collect();
        let fair_launches: u64 = self.fair_launch_angles.values().sum();
        let fair_angles: BTreeMap<_, _> = FAIR_ANGLE_BANDS
            .iter()
            .map(|&band| {
                let count = self
                    .fair_launch_angles
                    .get(band)
                    .copied()
                    .unwrap_or_default();
                let share = (fair_launches > 0).then(|| count as f64 / fair_launches as f64);
                (band, json!({"launches":count,"share_of_fair":share}))
            })
            .collect();
        let fair_hits = self
            .outcome_stages
            .get("fair_hit")
            .copied()
            .unwrap_or_default();
        let fair_non_hits = self
            .outcome_stages
            .get("fair_non_hit")
            .copied()
            .unwrap_or_default();
        let resolved_fair = fair_hits + fair_non_hits;
        let mut report = json!({
            "swings": self.swings, "by_swing_band": bands,
            "adjusted_contact_type_counts": self.contact_types,
            "outcome_stages": self.outcome_stages,
            "resolved_fair_swings": resolved_fair,
            "hits_per_resolved_fair_swing": (resolved_fair > 0).then(|| fair_hits as f64 / resolved_fair as f64),
            "baseline_to_trial_swing_bands": self.transitions,
            "baseline_hard_low_single_destinations": self.low_single_transitions,
            "baseline_5_to_10_deg_single_destinations": self.target_single_destinations,
            "fair_launches": fair_launches,
            "fair_launch_angle_distribution": fair_angles,
            "singles": self.singles, "singles_per_swing": rate(self.singles),
            "singles_5_to_10_deg": self.singles_5_to_10_deg,
            "singles_5_to_10_deg_per_swing": rate(self.singles_5_to_10_deg),
            "doubles": self.doubles, "triples": self.triples,
            "home_runs": self.home_runs, "home_runs_per_swing": rate(self.home_runs),
            "hard_fair_5_to_10_deg": self.hard_fair_5_to_10_deg,
            "outfield_direct_outs": self.outfield_direct_outs,
            "outfield_direct_outs_per_swing": rate(self.outfield_direct_outs),
            "retention_launches": self.retention_launches,
            "retention_at_least_80_pct": self.retention_at_least_80_pct,
            "retention_at_least_80_pct_per_swing": rate(self.retention_at_least_80_pct),
            "retention_at_least_95_pct": self.retention_at_least_95_pct,
            "retention_at_least_95_pct_per_swing": rate(self.retention_at_least_95_pct),
            "baseline_fair_retention_by_angle_deg": self.baseline_fair_retention_by_angle,
            "hard_low_singles": self.hard_low_singles, "hard_low_singles_per_swing": rate(self.hard_low_singles),
            "no_batting_result": self.no_batting_result,
            "hard_5_to_15_deg_infield": self.infield.report(),
        });
        if !self.double_fence.is_empty() {
            report["double_fence_diagnostics"] = json!(self.double_fence);
        }
        report
    }
}

#[derive(Default)]
struct PairedCounts {
    all: SwingCounts,
    contact_first: SwingCounts,
    line_drive: SwingCounts,
}

impl PairedCounts {
    fn observe(&mut self, intent: BattedBallIntent, baseline: IntentAimPlay, play: IntentAimPlay) {
        self.all.observe(baseline, play);
        match intent {
            BattedBallIntent::ContactFirst => &mut self.contact_first,
            BattedBallIntent::LineDrive => &mut self.line_drive,
        }
        .observe(baseline, play);
    }

    fn report(&self) -> Value {
        json!({
            "all": self.all.report(), "contact_first": self.contact_first.report(),
            "line_drive": self.line_drive.report(),
        })
    }
}

fn observe_paired(
    result: &GameResult,
    swings: Vec<IntentAimSwing>,
    pooled: &mut [PairedCounts],
    seed_counts: &mut [PairedCounts],
) -> Result<()> {
    let mut live: BTreeMap<_, _> = result
        .player_battings
        .iter()
        .filter(|b| {
            b.swing_decision
                .as_ref()
                .is_some_and(|decision| decision.swung)
        })
        .map(|b| (b.count_seq, b))
        .collect();
    for swing in swings {
        ensure!(
            swing.plays.len() == pooled.len(),
            "paired swing arm count mismatch"
        );
        let baseline = swing.plays[0];
        let recorded = live.remove(&swing.count_seq);
        ensure!(
            baseline.batting.outcome == recorded.map(|b| b.result)
                && recorded.is_none_or(|b| match baseline.batting.flight {
                    None => b.ball.launch_speed == 0.0,
                    Some((_, ev, angle)) =>
                        b.ball.launch_speed == ev && b.ball.launch_angle == angle,
                }),
            "baseline swing mismatch in game {}, count {}",
            result.id,
            swing.count_seq
        );
        for ((total, seed), play) in pooled
            .iter_mut()
            .zip(seed_counts.iter_mut())
            .zip(swing.plays)
        {
            total.observe(swing.intent, baseline, play);
            seed.observe(swing.intent, baseline, play);
        }
    }
    ensure!(
        live.is_empty(),
        "unpaired live swings in game {}",
        result.id
    );
    Ok(())
}

pub(super) fn run_speed(args: &Args, games: &[GameSchedule]) -> Result<Value> {
    const FACTORS: [f64; 3] = [1.0, 0.97, 0.94];
    run_replays(
        args,
        games,
        "ContactFirst swing speed",
        "contact_first_speed_factor",
        &FACTORS,
        "Paired live swing decisions from the same pitch, intent, count and pre-contact RNG state. Scale only ContactFirst bat speed before contact geometry. Timing displacement, the existing EV-dependent contact window, collision, flight, fielding and baserunning are recalculated. Later pitches follow the live baseline. Rates use the same swings, including whiffs, fouls and interrupted plays.",
        |simulate| with_contact_first_speed_audit(&FACTORS, simulate),
    )
}

pub(super) fn run_length(args: &Args, games: &[GameSchedule]) -> Result<Value> {
    const LOSSES: [f64; 3] = [0.0, 0.20, 0.35];
    run_replays(
        args,
        games,
        "Contact length retention",
        "max_additional_length_loss",
        &LOSSES,
        "Freeze contact geometry and classification, bat speed, launch angles and spin for both intents. Multiply the existing length retention by one minus the additional loss. Loss is zero through 2 cm from the sweet spot, increases by smoothstep to the arm maximum at 10 cm, and stays at that maximum beyond 10 cm. Recalculate flight, fielding and baserunning. Later pitches follow the live baseline. The retention histogram is grouped by baseline angle among baseline fair flights. Numeric keys are lower edges of 5 percentage point intervals; the 95 bin includes 100%. Retention uses both the thickness and length factors relative to the same pitch-plus-bat speed before either factor.",
        |simulate| {
            let (result, swings) = with_contact_length_audit(&LOSSES, simulate);
            for swing in &swings {
                let baseline = swing.plays[0].batting;
                for arm in &swing.plays[1..] {
                    let play = arm.batting;
                    if !matches!((baseline.flight, play.flight), (None, None))
                        && !matches!((baseline.flight, play.flight), (Some((_, before, angle)), Some((_, after, trial_angle))) if angle == trial_angle && after <= before)
                    {
                        return (
                            Err(anyhow::anyhow!(
                                "contact or launch angle changed, or exit speed increased at count {}",
                                swing.count_seq
                            )),
                            swings,
                        );
                    }
                }
            }
            (result, swings)
        },
    )
}

pub(super) fn run_timing_geometry(args: &Args, games: &[GameSchedule]) -> Result<Value> {
    let trials: [PitchBatTimingTrial; 6] = std::array::from_fn(|index| {
        let (contact_first_z_mm, contact_first_attack_deg) = [(0, 0), (8, 0), (0, 10)][index % 3];
        PitchBatTimingTrial {
            contact_first_z_mm,
            contact_first_attack_deg,
            use_path_mismatch: index >= 3,
            contact_first_vertical_bat_control_delta: 0.0,
        }
    });
    run_replays(
        args,
        games,
        "Pitch–bat timing geometry",
        "trial",
        &trials,
        "Replay realized swings from the same pitch, intent, count, and RNG position. ContactFirst uses the current aim, an 8 mm undercut, or a 10 degree steeper attack. Each runs with and without the added timing height error from the local pitch and bat velocities in the vertical plane. The calculation uses signed adapted timing error, plate velocity, and the trial attack angle. LineDrive keeps its live aim and attack angle. Existing horizontal timing displacement and contact rules still apply. Strong contact counts include any launch that retains at least 80% or 95% of the modeled pre-offset EV, including fouls. Outfield direct outs require a recorded batting out and a clean air catch by an outfielder. Divide by all swings. Later pitches follow the live baseline.",
        |simulate| with_pitch_bat_timing_audit(&trials, false, simulate),
    )
}

pub(super) fn run_vertical_bat_control(args: &Args, games: &[GameSchedule]) -> Result<Value> {
    let trials = [
        (0, false, 0.0),
        (0, true, 0.0),
        (10, true, 0.0),
        (10, true, -1.0),
    ]
    .map(
        |(
            contact_first_attack_deg,
            use_path_mismatch,
            contact_first_vertical_bat_control_delta,
        )| {
            PitchBatTimingTrial {
                contact_first_z_mm: 0,
                contact_first_attack_deg,
                use_path_mismatch,
                contact_first_vertical_bat_control_delta,
            }
        },
    );
    let mut destinations: BTreeMap<&str, BTreeMap<String, u64>> = BTreeMap::new();
    let mut report = run_replays(
        args,
        games,
        "ContactFirst vertical bat control",
        "trial",
        &trials,
        "Arm 0 verifies the live baseline. Compare arms 1–3 with the same pitch–bat timing geometry: current approach, ContactFirst attack +10 degrees, and attack +10 degrees with a 1-point bat_control reduction solely in the vertical pitch-offset correction. Stored bat_control, horizontal correction, timing correction, swing decision and later pitches follow the baseline. The destination table follows all control-arm doubles and home runs to their outcomes in the reduced-control arm. Double fence diagnostics classify official doubles by automatic award, wall contact before or after the first bounce, a clean fielding event before the projected wall contact, or no projected wall contact. Bounce and pickup gaps are the fence distance from home plate minus the point's distance from home plate at the same spray angle. Wall shortfall is fence height minus sampled ball height before the first bounce.",
        |simulate| {
            let (result, swings) = with_pitch_bat_timing_audit(&trials, true, simulate);
            for swing in &swings {
                if swing.intent != BattedBallIntent::ContactFirst {
                    continue;
                }
                let source = match swing.plays[2].batting.outcome {
                    Some(BattingResult::Double) => "Double",
                    Some(BattingResult::HomeRun) => "HomeRun",
                    _ => continue,
                };
                let replay = &swing.plays[3].batting;
                let destination = if replay.flight.is_none() {
                    "Whiff"
                } else {
                    replay
                        .outcome
                        .as_ref()
                        .map_or("NoBattingResult", AsRef::as_ref)
                };
                *destinations
                    .entry(source)
                    .or_default()
                    .entry(destination.to_owned())
                    .or_default() += 1;
            }
            (result, swings)
        },
    )?;
    report["plus_ten_extra_base_hit_destinations"] = json!(destinations);
    Ok(report)
}

pub(super) fn run_contact_approaches(args: &Args, games: &[GameSchedule]) -> Result<Value> {
    let live = IntentAimOffsets::default();
    let offsets = [
        live,
        IntentAimOffsets {
            contact_first_mm: live.contact_first_mm,
            line_drive_mm: live.contact_first_mm,
        },
        IntentAimOffsets {
            contact_first_mm: live.line_drive_mm,
            line_drive_mm: live.line_drive_mm,
        },
    ];
    let mut transitions: BTreeMap<&str, BTreeMap<&str, u64>> = BTreeMap::new();
    let mut quality = ContactApproachQuality::default();
    let mut two_strike = TwoStrikeApproach::default();
    let mut report = run_replays(
        args,
        games,
        "ContactFirst vs LineDrive",
        "approach",
        &["live", "contact_first", "line_drive"],
        "Every live swing is replayed with its original aim and with each approach's live vertical contact offset, before evaluating contact. Pitch, swing decision, batter, count, starting game state and pre-contact RNG are shared. Existing vertical contact policy, fielding and running are reapplied. Later pitches follow the live baseline. The all group combines all swings; contact_first and line_drive subgroups use the originally selected intent. Contact types use the adjusted contact classifier. A resolved fair swing has a non-foul flight and a non-foul recorded batting outcome; its hit rate includes home runs. Fair interrupted plays are kept in their own outcome stage.",
        |simulate| {
            let (result, swings, pitches) = with_contact_approach_audit(&offsets, simulate);
            let result = result.and_then(|result| {
                two_strike
                    .observe_game(pitches, &swings)
                    .with_context(|| format!("two-strike audit failed in game {}", result.id))?;
                Ok(result)
            });
            for swing in &swings {
                let contact_first = swing.plays[1].batting;
                let line_drive = swing.plays[2].batting;
                quality.observe(contact_first, line_drive);
                let source = outcome_stage(contact_first);
                let destination = outcome_stage(line_drive);
                *transitions
                    .entry(source)
                    .or_default()
                    .entry(destination)
                    .or_default() += 1;
            }
            (result, swings)
        },
    )?;
    report["contact_first_to_line_drive_outcome_stages"] = json!(transitions);
    report["paired_contact_quality"] = quality.report();
    report["two_strike_by_location"] = two_strike.report();
    Ok(report)
}

fn run_replays<const N: usize, V: serde::Serialize>(
    args: &Args,
    games: &[GameSchedule],
    name: &str,
    parameter: &str,
    values: &[V; N],
    definition: &str,
    mut audit: impl FnMut(
        &mut dyn FnMut() -> Result<GameResult>,
    ) -> (Result<GameResult>, Vec<IntentAimSwing>),
) -> Result<Value> {
    let empty = || std::array::from_fn::<_, N, _>(|_| PairedCounts::default());
    let report = |counts: &[PairedCounts]| {
        values
            .iter()
            .zip(counts)
            .map(|(value, summary)| json!({(parameter):value,"summary":summary.report()}))
            .collect::<Vec<_>>()
    };
    let mut pooled = empty();
    let mut reports = Vec::new();
    for &seed in &args.seeds {
        let mut per_seed = empty();
        for game in games {
            let (result, swings) = audit(&mut || {
                simulate_game(
                    game.clone(),
                    Box::new(RealRng::from_seed(derived_seed(seed, game.id))),
                    BattingStrategySettings::player_driven(),
                    Some(args.max_pitches),
                    false,
                )
            });
            let result = result.with_context(|| {
                format!("{name} replay failed in game {}, seed {seed}", game.id)
            })?;
            observe_paired(&result, swings, &mut pooled, &mut per_seed)?;
        }
        eprintln!("{name}, seed {seed}: {} games complete", games.len());
        reports.push(json!({"seed":seed.to_string(),"arms":report(&per_seed)}));
    }
    Ok(json!({"definition":definition,"pooled":report(&pooled),"reports":reports}))
}

pub(super) fn run(args: &Args, games: &[GameSchedule]) -> Result<Value> {
    let arms = arms(args);
    ensure!(
        arms.len() > 1,
        "at least one aim must differ from the baseline"
    );
    let offsets: Vec<_> = arms.iter().map(|&(_, offsets)| offsets).collect();
    let mut paired_totals: Vec<_> = offsets.iter().map(|_| PairedCounts::default()).collect();
    let mut paired_reports = Vec::new();
    let mut full_games = Vec::new();
    for (index, &(name, arm_offsets)) in arms.iter().enumerate() {
        let mut total = Counts::default();
        let mut reports = Vec::new();
        for &seed in &args.seeds {
            let mut per_seed = Counts::default();
            let mut teams = BTreeMap::new();
            let mut players = BTreeMap::new();
            let mut seed_paired: Vec<_> = if index == 0 {
                offsets.iter().map(|_| PairedCounts::default()).collect()
            } else {
                Vec::new()
            };
            for game in games {
                let simulate = || {
                    simulate_game(
                        game.clone(),
                        Box::new(RealRng::from_seed(derived_seed(seed, game.id))),
                        BattingStrategySettings::player_driven(),
                        Some(args.max_pitches),
                        false,
                    )
                };
                let (result, paired_swings) = if index == 0 {
                    with_intent_aim_audit(&offsets, simulate)
                } else {
                    (with_intent_aim_trial(arm_offsets, simulate).0, Vec::new())
                };
                let result = result.with_context(|| {
                    format!("intent aim {name}, seed {seed}, game {} failed", game.id)
                })?;
                if index == 0 {
                    observe_paired(&result, paired_swings, &mut paired_totals, &mut seed_paired)?;
                }
                accumulate(&mut per_seed, &mut teams, &mut players, game, &result)?;
            }
            if index == 0 {
                paired_reports.push(json!({
                    "seed":seed.to_string(),
                    "arms":arms.iter().zip(&seed_paired).map(|(&(name, _), counts)| (name, counts.report()))
                        .collect::<BTreeMap<_, _>>(),
                }));
            }
            reports.push(
                json!({"seed": seed.to_string(), "total": per_seed.vertical_contact_json(true)}),
            );
            total.merge(per_seed);
            eprintln!(
                "Intent aim {name}, seed {seed}: {} games complete",
                games.len()
            );
        }
        full_games.push(json!({
            "name":name, "aim_offsets":arm_offsets, "total":total.vertical_contact_json(true), "reports":reports,
        }));
    }
    Ok(json!({
        "definitions": {
            "paired_swings":"Every live baseline swing is replayed from the same pitch, selected intent, count, fielding and baserunning state, and pre-contact RNG position. Only the selected intent's world-vertical aim changes. The existing EV-dependent contact window is recomputed. The live baseline determines later swings.",
            "fair_launch_angle_distribution":"All non-foul simulated flights before fielding, including any exit speed and interrupted plays. Shares use the arm's fair flights as denominator. Angle intervals include their lower bound and exclude their upper bound.",
            "baseline_5_to_10_deg_single_destinations":"Every baseline official single at launch angle [5,10) degrees and any exit speed. Each arm counts its batting result first and then its fair angle band, foul flight, or no launch. A missing batting result is counted as NoBattingResult.",
            "bands":"Every swing has one band. Hard fair bands use non-foul flights at EV >=45 m/s and half-open angle ranges. Other fair includes downwards hard flights and slower fair flights. Rulings such as singles and home runs are counted separately. An interrupted fair play can lack a batting ruling.",
            "hard_5_to_15_deg_infield":"Fair InField flights at EV >=45 m/s and VLA [5,15) degrees. Fielding location uses the selected glove/pickup point and the engine's 50 m radius. Reach scans end at the estimated first 50 m crossing or known outfielder touch, whichever comes first. The scan uses live lane/distance filters, air risk and sampling, then relaxed filters and 10 ms sampling. These are clean opportunities. The first bounce is projected, including on air catches. Fielding selections before an interrupted baserunning play are included with NoBattingResult. Transitions include swings in the target population on either side.",
            "full_games":"All aim settings use the same schedule, source rosters and initial game seeds. Each arm advances its own games, so decisions and later situations may diverge. Singles and home runs are measured per PA.",
        },
        "paired_swings":{
            "pooled":arms.iter().zip(&paired_totals).map(|(&(name, _), counts)| (name, counts.report()))
                .collect::<BTreeMap<_, _>>(),
            "reports":paired_reports,
        },
        "full_games":full_games,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn aim_settings_cross_the_two_intents_and_deduplicate_controls() {
        let parse = |extra: &[&str]| {
            let mut args = vec!["check", "--season", "2026", "--intent-aim-comparison"];
            args.extend_from_slice(extra);
            Args::try_parse_from(args).unwrap()
        };
        let configs = arms(&parse(&[]));
        let millimeters: Vec<_> = configs
            .iter()
            .map(|(_, offset)| (offset.contact_first_mm, offset.line_drive_mm))
            .collect();
        assert_eq!(millimeters, [(0, 20), (8, 20), (0, 32), (8, 32)]);
        let control = arms(&parse(&[
            "--contact-first-aim-mm",
            "-5",
            "--line-drive-aim-mm",
            "20",
        ]));
        assert_eq!(control.len(), 2);
        assert_eq!(control[1].1.contact_first_mm, -5);
        assert!(
            Args::try_parse_from(["check", "--season", "2026", "--contact-first-aim-mm", "8"])
                .is_err()
        );
    }

    #[test]
    fn target_single_destinations_cover_all_speeds_and_keep_fouls_out_of_fair_angles() {
        let replay = |outcome, flight| IntentAimPlay {
            batting: SwingReplay {
                exit_speed_retention: None,
                length_offset_m: 0.0,
                thickness_offset_m: 0.0,
                outfield_direct_out: false,
                contact_type:
                    saberbb::domain::resolver::batting_resolver::SwingContactType::SolidContact,
                double_fence: None,
                outcome,
                flight,
            },
            infield: None,
        };
        let single = replay(
            Some(BattingResult::Single),
            Some((OutboundResult::InField, 37.0, 5.0)),
        );
        let boundary = replay(
            Some(BattingResult::Single),
            Some((OutboundResult::InField, 50.0, 10.0)),
        );
        let mut counts = SwingCounts::default();
        counts.observe(
            single,
            replay(
                Some(BattingResult::Double),
                Some((OutboundResult::InField, 46.0, 20.0)),
            ),
        );
        counts.observe(
            single,
            replay(
                Some(BattingResult::Foul),
                Some((OutboundResult::Foul, 48.0, 9.0)),
            ),
        );
        counts.observe(single, replay(None, None));
        counts.observe(boundary, boundary);
        assert_eq!(counts.target_single_destinations["Double"]["20_to_40"], 1);
        assert_eq!(counts.target_single_destinations["Foul"]["foul_flight"], 1);
        assert_eq!(
            counts.target_single_destinations["NoBattingResult"]["no_launch"],
            1
        );
        assert!(!counts.target_single_destinations.contains_key("Single"));
        let report = counts.report();
        assert_eq!(report["fair_launches"], 2);
        assert_eq!(report["resolved_fair_swings"], 2);
        assert_eq!(report["hits_per_resolved_fair_swing"], 1.0);
        assert_eq!(report["outcome_stages"]["foul"], 1);
        assert_eq!(report["outcome_stages"]["whiff"], 1);
        assert_eq!(
            report["fair_launch_angle_distribution"]["10_to_20"]["share_of_fair"],
            0.5
        );
        assert_eq!(
            report["fair_launch_angle_distribution"]["5_to_10"]["launches"],
            0
        );
    }

    #[test]
    fn paired_ground_ball_single_can_move_to_a_foul_or_a_high_angle_home_run() {
        let replay = |batting| IntentAimPlay {
            batting,
            infield: None,
        };
        let single = replay(SwingReplay {
            exit_speed_retention: None,
            length_offset_m: 0.0,
            thickness_offset_m: 0.0,
            outfield_direct_out: false,
            contact_type:
                saberbb::domain::resolver::batting_resolver::SwingContactType::SolidContact,
            double_fence: None,
            outcome: Some(BattingResult::Single),
            flight: Some((OutboundResult::InField, 45.0, 0.0)),
        });
        let mut counts = SwingCounts::default();
        counts.observe(
            single,
            replay(SwingReplay {
                exit_speed_retention: None,
                length_offset_m: 0.0,
                thickness_offset_m: 0.0,
                outfield_direct_out: false,
                contact_type:
                    saberbb::domain::resolver::batting_resolver::SwingContactType::SolidContact,
                double_fence: None,
                outcome: Some(BattingResult::Foul),
                flight: Some((OutboundResult::Foul, 48.0, 31.0)),
            }),
        );
        counts.observe(
            single,
            replay(SwingReplay {
                exit_speed_retention: None,
                length_offset_m: 0.0,
                thickness_offset_m: 0.0,
                outfield_direct_out: false,
                contact_type:
                    saberbb::domain::resolver::batting_resolver::SwingContactType::SolidContact,
                double_fence: None,
                outcome: Some(BattingResult::HomeRun),
                flight: Some((OutboundResult::HomeRun, 45.0, 30.0)),
            }),
        );
        assert_eq!(counts.swings, 2);
        assert_eq!(counts.bands["foul"], 1);
        assert_eq!(counts.bands["hard_fair_30_to_40_deg"], 1);
        assert_eq!(counts.transitions[HARD_LOW]["foul"], 1);
        assert_eq!(
            counts.low_single_transitions["hard_fair_30_to_40_deg:HomeRun"],
            1
        );
        assert_eq!((counts.hard_low_singles, counts.home_runs), (0, 1));
    }
}
