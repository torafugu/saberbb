use super::{Args, GameSchedule, derived_seed, simulate_game};
use anyhow::{Context, Result, ensure};
use saberbb::domain::{
    random_provider::RealRng,
    shared::game_state::{
        FixedHorizontalScan, POPUP_FIXED_X_STEP_M, POPUP_HLA_RETENTION_PCT, POPUP_SCAN_INFIELD_M,
        POPUP_SCAN_VLA_DEG, POPUP_SCAN_X_MM, POPUP_SCAN_Z_MM, POPUP_ZERO_SPEED_TOLERANCE_MS,
        PopupHlaTrial, PopupZoneLaunch, PopupZonePlay, PopupZoneSwing,
        with_fixed_horizontal_popup_audit, with_popup_hla_sweep_audit, with_popup_zone_audit,
    },
    strategy::batting_strategy::{BattedBallIntent, BattingStrategySettings},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const WIDTH: usize = POPUP_SCAN_X_MM.len();
const CELLS: usize = WIDTH * POPUP_SCAN_Z_MM.len();

fn nearest_cell(offset_m: f64, centers_mm: &[i32]) -> Option<usize> {
    let step = f64::from(centers_mm[1] - centers_mm[0]);
    let index = ((offset_m * 1000.0 - f64::from(centers_mm[0])) / step + 0.5).floor();
    (index >= 0.0 && index < centers_mm.len() as f64).then_some(index as usize)
}

fn has_high_vla(launch: PopupZoneLaunch) -> bool {
    launch.vla_deg > POPUP_SCAN_VLA_DEG && launch.exit_speed_ms > POPUP_ZERO_SPEED_TOLERANCE_MS
}

fn is_popup_launch(launch: PopupZoneLaunch) -> bool {
    has_high_vla(launch)
        && !launch.foul
        && (launch.projected_infield_bounce || launch.clean_infielder_air_out)
}

fn is_popup(play: PopupZonePlay) -> bool {
    play.launch.is_some_and(is_popup_launch)
}

#[derive(Clone, Copy, Debug)]
struct PopupDistance {
    cell: usize,
    dx_mm: f64,
    dz_mm: f64,
    distance_mm: f64,
}

fn nearest_popup(swing: &PopupZoneSwing) -> Option<PopupDistance> {
    swing
        .grid
        .iter()
        .copied()
        .enumerate()
        .filter(|&(_, play)| is_popup(play))
        .map(|(cell, play)| {
            let dx_mm = (play.x_m - swing.baseline.x_m) * 1000.0;
            let dz_mm = (play.z_m - swing.baseline.z_m) * 1000.0;
            PopupDistance {
                cell,
                dx_mm,
                dz_mm,
                distance_mm: dx_mm.hypot(dz_mm),
            }
        })
        .min_by(|a, b| {
            a.distance_mm
                .total_cmp(&b.distance_mm)
                .then(a.cell.cmp(&b.cell))
        })
}

const DISTANCE_BANDS_MM: [f64; 5] = [5.0, 10.0, 20.0, 40.0, 80.0];
const DISTANCE_BAND_NAMES: [&str; 6] = [
    "up_to_5",
    "over_5_to_10",
    "over_10_to_20",
    "over_20_to_40",
    "over_40_to_80",
    "over_80",
];

#[derive(Default)]
struct Distances {
    sum: f64,
    sum_abs: f64,
    negative: u64,
    positive: u64,
    magnitude_counts: [u64; DISTANCE_BAND_NAMES.len()],
}

impl Distances {
    fn observe(&mut self, mm: f64) {
        self.sum += mm;
        self.sum_abs += mm.abs();
        self.negative += u64::from(mm < 0.0);
        self.positive += u64::from(mm > 0.0);
        let band = DISTANCE_BANDS_MM
            .iter()
            .position(|&upper| mm.abs() <= upper + 1e-9)
            .unwrap_or(DISTANCE_BANDS_MM.len());
        self.magnitude_counts[band] += 1;
    }

    fn report(&self, count: u64) -> Value {
        let mean = |sum: f64| (count > 0).then(|| sum / count as f64);
        let bands: BTreeMap<_, _> = DISTANCE_BAND_NAMES
            .into_iter()
            .zip(self.magnitude_counts)
            .collect();
        json!({
            "mean_signed_mm": mean(self.sum),
            "mean_absolute_mm": mean(self.sum_abs),
            "negative": self.negative,
            "positive": self.positive,
            "zero": count - self.negative - self.positive,
            "by_absolute_mm": bands,
        })
    }
}

#[derive(Default)]
struct DistancesToPopup {
    swings: u64,
    x: Distances,
    z: Distances,
    euclidean: Distances,
}

impl DistancesToPopup {
    fn observe(&mut self, nearest: PopupDistance) {
        self.swings += 1;
        self.x.observe(nearest.dx_mm);
        self.z.observe(nearest.dz_mm);
        self.euclidean.observe(nearest.distance_mm);
    }

    fn report(&self) -> Value {
        json!({
            "swings": self.swings,
            "horizontal": self.x.report(self.swings),
            "vertical": self.z.report(self.swings),
            "euclidean": self.euclidean.report(self.swings),
        })
    }
}

#[derive(Default)]
struct LaunchCounts {
    count: u64,
    exit_speed_sum: f64,
    vla_sum: f64,
    abs_hla_sum: f64,
}

impl LaunchCounts {
    fn observe(&mut self, ev_ms: f64, vla_deg: f64, hla_deg: f64) {
        self.count += 1;
        self.exit_speed_sum += ev_ms;
        self.vla_sum += vla_deg;
        self.abs_hla_sum += hla_deg.abs();
    }

    fn report(&self) -> Value {
        let mean = |sum: f64| (self.count > 0).then(|| sum / self.count as f64);
        json!({
            "count": self.count,
            "mean_exit_speed_ms": mean(self.exit_speed_sum),
            "mean_vla_deg": mean(self.vla_sum),
            "mean_abs_hla_deg": mean(self.abs_hla_sum),
        })
    }
}

#[derive(Default)]
struct CellCounts {
    swings: u64,
    outcomes: BTreeMap<String, u64>,
    launches: LaunchCounts,
    high_foul: u64,
    zero_speed_high_angle: u64,
    high_foul_infielder_air_out: u64,
    high_fair: u64,
    projected_infield_bounce: u64,
    clean_infielder_air_out: u64,
    popups: LaunchCounts,
}

impl CellCounts {
    fn observe(&mut self, play: PopupZonePlay) {
        self.swings += 1;
        let outcome = play
            .outcome
            .as_ref()
            .map_or("NoBattingResult", |r| r.as_ref());
        *self.outcomes.entry(outcome.to_owned()).or_default() += 1;
        let Some(launch) = play.launch else { return };
        self.launches
            .observe(launch.exit_speed_ms, launch.vla_deg, launch.hla_deg);
        if launch.vla_deg <= POPUP_SCAN_VLA_DEG {
            return;
        }
        if !has_high_vla(launch) {
            self.zero_speed_high_angle += 1;
            return;
        }
        if launch.foul {
            self.high_foul += 1;
            self.high_foul_infielder_air_out += u64::from(launch.foul_infielder_air_out);
            return;
        }
        self.high_fair += 1;
        self.projected_infield_bounce += u64::from(launch.projected_infield_bounce);
        self.clean_infielder_air_out += u64::from(launch.clean_infielder_air_out);
        if is_popup(play) {
            self.popups
                .observe(launch.exit_speed_ms, launch.vla_deg, launch.hla_deg);
        }
    }

    fn report(&self) -> Value {
        json!({
            "swings": self.swings,
            "outcomes": self.outcomes,
            "launches": self.launches.report(),
            "vla_above_threshold_foul": self.high_foul,
            "zero_speed_above_threshold": self.zero_speed_high_angle,
            "high_foul_clean_infielder_air_out": self.high_foul_infielder_air_out,
            "vla_above_threshold_fair": self.high_fair,
            "high_fair_projected_infield_bounce": self.projected_infield_bounce,
            "high_fair_clean_infielder_air_out": self.clean_infielder_air_out,
            "fair_infield_popups": self.popups.report(),
        })
    }
}

struct ZoneCounts {
    grid: [CellCounts; CELLS],
    baseline: [CellCounts; CELLS],
    baseline_total: CellCounts,
    baseline_outside_grid: CellCounts,
    distances: DistancesToPopup,
    from_live_contact: DistancesToPopup,
    from_live_whiff: DistancesToPopup,
    nearest_target_counts: [u64; CELLS],
}

impl Default for ZoneCounts {
    fn default() -> Self {
        Self {
            grid: std::array::from_fn(|_| CellCounts::default()),
            baseline: std::array::from_fn(|_| CellCounts::default()),
            baseline_total: CellCounts::default(),
            baseline_outside_grid: CellCounts::default(),
            distances: DistancesToPopup::default(),
            from_live_contact: DistancesToPopup::default(),
            from_live_whiff: DistancesToPopup::default(),
            nearest_target_counts: [0; CELLS],
        }
    }
}

impl ZoneCounts {
    fn observe(&mut self, swing: &PopupZoneSwing, nearest: Option<PopupDistance>) {
        self.baseline_total.observe(swing.baseline);
        let x = nearest_cell(swing.baseline.x_m, &POPUP_SCAN_X_MM);
        let z = nearest_cell(swing.baseline.z_m, &POPUP_SCAN_Z_MM);
        if let (Some(x), Some(z)) = (x, z) {
            self.baseline[z * WIDTH + x].observe(swing.baseline);
        } else {
            self.baseline_outside_grid.observe(swing.baseline);
        }
        for (cell, &play) in self.grid.iter_mut().zip(&swing.grid) {
            cell.observe(play);
        }
        if let Some(nearest) = nearest {
            self.distances.observe(nearest);
            self.nearest_target_counts[nearest.cell] += 1;
            if swing.baseline.launch.is_some() {
                self.from_live_contact.observe(nearest);
            } else {
                self.from_live_whiff.observe(nearest);
            }
        }
    }

    fn report(&self) -> Value {
        let cells: Vec<_> = self
            .grid
            .iter()
            .zip(&self.baseline)
            .enumerate()
            .map(|(i, (grid, baseline))| {
                json!({
                    "x_mm": POPUP_SCAN_X_MM[i % WIDTH],
                    "z_mm": POPUP_SCAN_Z_MM[i / WIDTH],
                    "forced_offset": grid.report(),
                    "live_nearest_cell": baseline.report(),
                    "swings_with_nearest_popup_here": self.nearest_target_counts[i],
                })
            })
            .collect();
        json!({
            "live_total": self.baseline_total.report(),
            "live_outside_grid": self.baseline_outside_grid.report(),
            "swings_with_any_grid_popup": self.distances.swings,
            "swings_without_grid_popup": self.baseline_total.swings - self.distances.swings,
            "distance_to_nearest_grid_popup": {
                "all": self.distances.report(),
                "from_live_contact": self.from_live_contact.report(),
                "from_live_whiff": self.from_live_whiff.report(),
            },
            "cells": cells,
        })
    }
}

const HLA_OUTCOMES: [&str; 3] = ["high_foul", "fair_infield_popup", "other_high_fair"];

fn hla_outcome(launch: PopupZoneLaunch) -> usize {
    if launch.foul {
        0
    } else if is_popup_launch(launch) {
        1
    } else {
        2
    }
}

#[derive(Default)]
struct HlaControlCounts {
    pairs: [[u64; HLA_OUTCOMES.len()]; HLA_OUTCOMES.len()],
    swings_with_popup: u64,
    swings_with_paired_foul_to_popup: u64,
    swings_by_usual_scan_outcome: BTreeMap<&'static str, [u64; 2]>,
}

impl HlaControlCounts {
    fn observe(
        &mut self,
        usual_scan_outcome: &'static str,
        plays: &[PopupZonePlay],
        arm: usize,
        retention_pct: u8,
    ) {
        let mut any_popup = false;
        let mut paired_foul_to_popup = false;
        for play in plays {
            let Some(live) = play.launch.filter(|&launch| has_high_vla(launch)) else {
                continue;
            };
            let trial = if retention_pct == 100 {
                Some(live)
            } else {
                play.hla_trial_launches[arm]
            };
            if let Some(control) = trial {
                let from = hla_outcome(live);
                let to = hla_outcome(control);
                self.pairs[from][to] += 1;
                any_popup |= to == 1;
                paired_foul_to_popup |= from == 0 && to == 1;
            }
        }
        self.swings_with_popup += u64::from(any_popup);
        self.swings_with_paired_foul_to_popup += u64::from(paired_foul_to_popup);
        self.swings_by_usual_scan_outcome
            .entry(usual_scan_outcome)
            .or_default()[usize::from(any_popup)] += 1;
    }

    fn sweep_report(&self, retention_pct: u8) -> Value {
        let [foul, popup, other_fair] = self.pairs;
        json!({
            "retained_rebound_pct": retention_pct,
            "summary": {
                "high_foul_positions": foul[0] + popup[0] + other_fair[0],
                "foul_to_popup_at_same_position": foul[1],
                "standard_fair_positions": popup.iter().sum::<u64>() + other_fair.iter().sum::<u64>(),
                "standard_fair_positions_still_fair": popup[1] + popup[2] + other_fair[1] + other_fair[2],
                "standard_popup_positions": popup.iter().sum::<u64>(),
                "standard_popup_positions_still_popup": popup[1],
            },
            "comparison": self.report(),
        })
    }

    fn report(&self) -> Value {
        let pairs: BTreeMap<_, _> = HLA_OUTCOMES
            .into_iter()
            .zip(self.pairs)
            .map(|(from, row)| {
                (
                    from,
                    HLA_OUTCOMES
                        .into_iter()
                        .zip(row)
                        .collect::<BTreeMap<_, _>>(),
                )
            })
            .collect();
        let swings_by_outcome: BTreeMap<_, _> = self
            .swings_by_usual_scan_outcome
            .iter()
            .map(|(&category, &[without, with])| {
                (
                    category,
                    json!({"swings": without + with, "with_control_popup": with}),
                )
            })
            .collect();
        json!({
            "paired_high_angle_positions": pairs,
            "swings_with_any_control_popup": self.swings_with_popup,
            "swings_with_foul_to_popup_at_same_position": self.swings_with_paired_foul_to_popup,
            "swings_by_usual_scan_outcome": swings_by_outcome,
        })
    }
}

#[derive(Default)]
struct FixedHorizontalCounts {
    swings: u64,
    already_popup: u64,
    new_popup: u64,
    swings_with_high_foul_infielder_air_out: u64,
    offset_to_first_popup: Distances,
    limits_and_outcomes: BTreeMap<&'static str, u64>,
    new_popup_launches: LaunchCounts,
    hla_trials: [HlaControlCounts; POPUP_HLA_RETENTION_PCT.len()],
}

impl FixedHorizontalCounts {
    fn observe(
        &mut self,
        baseline: PopupZonePlay,
        scan: &FixedHorizontalScan,
        hla_trial: PopupHlaTrial,
    ) {
        use FixedHorizontalScan::*;
        self.swings += 1;
        let is_high_foul_air_out = |play: &PopupZonePlay| {
            play.launch
                .is_some_and(|launch| has_high_vla(launch) && launch.foul_infielder_air_out)
        };
        let has_high_foul_air_out = is_high_foul_air_out(&baseline)
            || matches!(scan, HighVlaSamples(plays) if plays.iter().any(is_high_foul_air_out));
        self.swings_with_high_foul_infielder_air_out += u64::from(has_high_foul_air_out);
        let category = match scan {
            _ if is_popup(baseline) => {
                self.already_popup += 1;
                "already_popup"
            }
            NoContactAtOrBelowLiveBat => "no_contact_at_or_below_live_bat",
            VerticalDeflectionCap => "vertical_deflection_cap",
            ThicknessEndsBeforeHighVla => "thickness_ends_before_high_vla",
            LengthEndsBeforeHighVla => "length_ends_before_high_vla",
            HighVlaSamples(plays) => {
                if let Some(play) = plays.iter().find(|&&play| is_popup(play)) {
                    let launch = play.launch.expect("popup has a launch");
                    self.new_popup += 1;
                    self.offset_to_first_popup
                        .observe((play.z_m - baseline.z_m) * 1000.0);
                    self.new_popup_launches.observe(
                        launch.exit_speed_ms,
                        launch.vla_deg,
                        launch.hla_deg,
                    );
                    "new_popup"
                } else {
                    let high = || {
                        plays
                            .iter()
                            .filter_map(|play| play.launch)
                            .filter(|&launch| has_high_vla(launch))
                    };
                    if high().any(|launch| !launch.foul && launch.projected_outfield_bounce) {
                        "high_fair_outfield_bounce_without_popup"
                    } else if high().any(|launch| !launch.foul) {
                        "high_fair_other_without_popup"
                    } else if high().any(|launch| launch.foul) {
                        "high_foul_only"
                    } else if plays.iter().filter_map(|play| play.launch).any(|launch| {
                        launch.vla_deg > POPUP_SCAN_VLA_DEG
                            && launch.exit_speed_ms <= POPUP_ZERO_SPEED_TOLERANCE_MS
                    }) {
                        "zero_speed_high_angle_only"
                    } else {
                        "no_high_contact_at_sampled_positions"
                    }
                }
            }
        };
        *self.limits_and_outcomes.entry(category).or_default() += 1;
        let plays: &[PopupZonePlay] = match scan {
            HighVlaSamples(plays) => plays,
            _ => &[],
        };
        for (arm, &retention) in hla_trial.retention_percentages().iter().enumerate() {
            self.hla_trials[arm].observe(category, plays, arm, retention);
        }
    }

    fn report(&self, hla_trial: PopupHlaTrial) -> Value {
        let mut report = json!({
            "swings": self.swings,
            "already_popup": self.already_popup,
            "new_popup": self.new_popup,
            "swings_with_high_foul_infielder_air_out": self.swings_with_high_foul_infielder_air_out,
            "additional_z_to_first_new_popup": self.offset_to_first_popup.report(self.new_popup),
            "first_new_popup_launches": self.new_popup_launches.report(),
            "limits_and_outcomes": self.limits_and_outcomes,
        });
        match hla_trial {
            PopupHlaTrial::Disabled => {}
            PopupHlaTrial::Control => {
                report["hla_without_contact_normal"] = self.hla_trials[0].report();
            }
            PopupHlaTrial::Sweep => {
                report["hla_rebound_sweep"] = json!(
                    self.hla_trials
                        .iter()
                        .zip(POPUP_HLA_RETENTION_PCT)
                        .map(|(counts, retention)| counts.sweep_report(retention))
                        .collect::<Vec<_>>()
                );
            }
        }
        report
    }
}

#[derive(Default)]
struct FixedHorizontalGroup {
    all: FixedHorizontalCounts,
    from_live_contact: FixedHorizontalCounts,
    from_live_whiff: FixedHorizontalCounts,
}

impl FixedHorizontalGroup {
    fn observe(
        &mut self,
        baseline: PopupZonePlay,
        scan: &FixedHorizontalScan,
        hla_trial: PopupHlaTrial,
    ) {
        self.all.observe(baseline, scan, hla_trial);
        if baseline.launch.is_some() {
            self.from_live_contact.observe(baseline, scan, hla_trial);
        } else {
            self.from_live_whiff.observe(baseline, scan, hla_trial);
        }
    }

    fn report(&self, hla_trial: PopupHlaTrial) -> Value {
        json!({
            "all": self.all.report(hla_trial),
            "from_live_contact": self.from_live_contact.report(hla_trial),
            "from_live_whiff": self.from_live_whiff.report(hla_trial),
        })
    }
}

pub(super) fn run(args: &Args, games: &[GameSchedule]) -> Result<Value> {
    let hla_trial = if args.popup_hla_sweep {
        PopupHlaTrial::Sweep
    } else if args.popup_hla_control {
        PopupHlaTrial::Control
    } else {
        PopupHlaTrial::Disabled
    };
    let mut all = ZoneCounts::default();
    let mut contact_first = ZoneCounts::default();
    let mut line_drive = ZoneCounts::default();
    let mut fixed_all = FixedHorizontalGroup::default();
    let mut fixed_contact_first = FixedHorizontalGroup::default();
    let mut fixed_line_drive = FixedHorizontalGroup::default();
    for &seed in &args.seeds {
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
            let (result, swings) = if args.popup_hla_sweep {
                with_popup_hla_sweep_audit(simulate)
            } else if args.popup_fixed_horizontal {
                with_fixed_horizontal_popup_audit(args.popup_hla_control, simulate)
            } else {
                with_popup_zone_audit(simulate)
            };
            let result = result.with_context(|| {
                format!("popup zone replay failed in game {}, seed {seed}", game.id)
            })?;
            let mut live: BTreeMap<_, _> = result
                .player_battings
                .iter()
                .filter(|row| {
                    row.swing_decision
                        .as_ref()
                        .is_some_and(|decision| decision.swung)
                })
                .map(|row| (row.count_seq, row))
                .collect();
            for swing in swings {
                let recorded = live.remove(&swing.count_seq);
                ensure!(
                    swing.baseline.outcome == recorded.map(|row| row.result)
                        && recorded.is_none_or(|row| match swing.baseline.launch {
                            Some(launch) =>
                                launch.exit_speed_ms == row.ball.launch_speed
                                    && launch.vla_deg == row.ball.launch_angle,
                            None => row.ball.launch_speed == 0.0,
                        }),
                    "popup zone baseline mismatch in game {}, count {}",
                    game.id,
                    swing.count_seq
                );
                if args.popup_fixed_horizontal {
                    let scan = swing
                        .fixed_horizontal
                        .as_ref()
                        .context("missing fixed horizontal popup scan")?;
                    fixed_all.observe(swing.baseline, scan, hla_trial);
                    match swing.intent {
                        BattedBallIntent::ContactFirst => &mut fixed_contact_first,
                        BattedBallIntent::LineDrive => &mut fixed_line_drive,
                    }
                    .observe(swing.baseline, scan, hla_trial);
                    continue;
                }
                ensure!(swing.grid.len() == CELLS, "popup zone grid size mismatch");
                let nearest = nearest_popup(&swing);
                all.observe(&swing, nearest);
                match swing.intent {
                    BattedBallIntent::ContactFirst => &mut contact_first,
                    BattedBallIntent::LineDrive => &mut line_drive,
                }
                .observe(&swing, nearest);
            }
            ensure!(live.is_empty(), "unpaired live swings in game {}", game.id);
        }
        eprintln!(
            "Popup contact zone, seed {seed}: {} games complete",
            games.len()
        );
    }
    if args.popup_fixed_horizontal {
        let mut report = json!({
            "definition": "Hold each swing's realized world X fixed. Increase Z (ball minus bat) from its live value. Intersect the legal length and thickness interval with the region above the VLA threshold, then replay the intersection at equally spaced points no more than the reported recovery step apart, including endpoints. A popup is a fair launch above the VLA threshold with a projected fair infield first bounce or a clean air out by an infielder in fair territory. Air-out territory uses the actual interception. An angle on a zero-speed contact is excluded from high launches. The foul air-out counter includes any sampled position or the live position and can overlap outcome groups. Outfield bounce means a projected fair first bounce at or beyond the infield radius. The outcome groups are exclusive: a new popup takes precedence, followed by high fair outfield bounce, other high fair, then high foul. Additional Z is measured to the first sampled popup; narrow intervals between samples can be missed. This is a geometric replay, independent of a batter's ability to move that far.",
            "vla_threshold_deg": POPUP_SCAN_VLA_DEG,
            "zero_speed_tolerance_ms": POPUP_ZERO_SPEED_TOLERANCE_MS,
            "infield_radius_m": POPUP_SCAN_INFIELD_M,
            "maximum_z_sample_step_mm": POPUP_FIXED_X_STEP_M * 1000.0,
            "all": fixed_all.report(hla_trial),
            "contact_first": fixed_contact_first.report(hla_trial),
            "line_drive": fixed_line_drive.report(hla_trial),
        });
        if args.popup_hla_control {
            report["hla_control_definition"] = json!(
                "At every scanned contact with nonzero EV and VLA above the threshold, remove only the signed contact-normal contribution to launch HLA. Keep the measured EV, VLA and spin and replay fielding with the same pre-fielding RNG state. Position transition counts use the same Z, while per-swing results allow any sampled Z."
            );
        }
        if args.popup_hla_sweep {
            report["hla_sweep_definition"] = json!(
                "Scale only the signed contact-normal HLA contribution at each matched high-angle contact. Reuse live EV, VLA, spin and pre-fielding RNG. The 100% row pairs the standard result with itself. Position counts use the same Z, while a swing reaches a popup if any sampled Z qualifies."
            );
        }
        return Ok(report);
    }
    Ok(json!({
        "definition": "Replace final world X and Z offsets at contact while retaining the live pitch, swing angles, timing, speed and starting RNG state. Positive Z puts the bat below the ball. A popup is a fair launch strictly above the VLA threshold with a projected first bounce in fair territory inside the infield radius or a clean air out by an infielder in fair territory. Air-out territory uses the actual interception. An angle on a zero-speed contact is excluded from high launches. Both subcriteria are reported and can overlap. Live offsets are binned into the nearest grid center, with half-step outer boundaries and ties assigned upward. Forced grid cells use the same swings and represent geometric opportunities, not expected outcome frequencies.",
        "nearest_grid_popup_definition": "For each swing, minimize Euclidean distance in world X/Z from the exact ordinary position to a grid replay that produced a fair infield popup. Ties use the first grid cell (increasing Z, then X). Signed displacements equal grid minus live. Include ordinary positions outside the grid and exclude swings with no popup on the scanned grid from distance distributions. Distances are to the tested grid, including when the live swing was already a popup.",
        "vla_threshold_deg": POPUP_SCAN_VLA_DEG,
        "zero_speed_tolerance_ms": POPUP_ZERO_SPEED_TOLERANCE_MS,
        "infield_radius_m": POPUP_SCAN_INFIELD_M,
        "x_grid_mm": POPUP_SCAN_X_MM,
        "z_grid_mm": POPUP_SCAN_Z_MM,
        "all": all.report(),
        "contact_first": contact_first.report(),
        "line_drive": line_drive.report(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_foul_to_popup_at_the_same_position_and_by_swing() {
        let foul = PopupZoneLaunch {
            exit_speed_ms: 28.0,
            vla_deg: 60.0,
            hla_deg: 75.0,
            foul: true,
            projected_infield_bounce: false,
            projected_outfield_bounce: false,
            clean_infielder_air_out: false,
            foul_infielder_air_out: false,
        };
        let popup = PopupZoneLaunch {
            hla_deg: 20.0,
            foul: false,
            projected_infield_bounce: true,
            ..foul
        };
        let play = PopupZonePlay {
            x_m: 0.0,
            z_m: 0.08,
            outcome: None,
            launch: Some(foul),
            hla_trial_launches: [Some(popup); POPUP_HLA_RETENTION_PCT.len() - 1],
        };
        let mut result = FixedHorizontalCounts::default();
        result.observe(
            PopupZonePlay {
                z_m: 0.0,
                launch: None,
                hla_trial_launches: [None; POPUP_HLA_RETENTION_PCT.len() - 1],
                ..play
            },
            &FixedHorizontalScan::HighVlaSamples(vec![play, play]),
            PopupHlaTrial::Sweep,
        );
        assert_eq!(result.hla_trials[0].pairs[0][1], 2);
        assert_eq!(result.hla_trials[3].pairs[0][1], 2);
        assert_eq!(result.hla_trials[4].pairs[0][0], 2);
        assert_eq!(result.hla_trials[0].swings_with_paired_foul_to_popup, 1);
        assert_eq!(
            result.hla_trials[0].swings_by_usual_scan_outcome["high_foul_only"],
            [0, 1]
        );
    }

    #[test]
    fn zero_speed_contacts_do_not_make_popups() {
        let launch = PopupZoneLaunch {
            exit_speed_ms: 0.0,
            vla_deg: 60.0,
            hla_deg: 175.0,
            foul: false,
            projected_infield_bounce: true,
            projected_outfield_bounce: false,
            clean_infielder_air_out: false,
            foul_infielder_air_out: false,
        };
        let play = PopupZonePlay {
            x_m: 0.0,
            z_m: 0.08,
            launch: Some(launch),
            hla_trial_launches: [None; POPUP_HLA_RETENTION_PCT.len() - 1],
            outcome: None,
        };
        assert!(!is_popup(play));
        assert!(!has_high_vla(PopupZoneLaunch {
            exit_speed_ms: 3e-29,
            ..launch
        }));
        assert!(has_high_vla(PopupZoneLaunch {
            exit_speed_ms: 0.001,
            ..launch
        }));
        let mut cell = CellCounts::default();
        cell.observe(play);
        assert_eq!(cell.zero_speed_high_angle, 1);
        assert_eq!(cell.high_fair, 0);
        let mut fixed = FixedHorizontalCounts::default();
        fixed.observe(
            PopupZonePlay {
                launch: None,
                ..play
            },
            &FixedHorizontalScan::HighVlaSamples(vec![play]),
            PopupHlaTrial::Disabled,
        );
        assert_eq!(
            fixed.limits_and_outcomes.get("zero_speed_high_angle_only"),
            Some(&1)
        );
    }

    #[test]
    fn finds_nearest_popup_from_exact_position_and_skips_non_popups() {
        let live = PopupZonePlay {
            x_m: -0.24,
            z_m: 0.02,
            launch: None,
            hla_trial_launches: [None; POPUP_HLA_RETENTION_PCT.len() - 1],
            outcome: None,
        };
        let popup = |x_m, z_m, foul| PopupZonePlay {
            x_m,
            z_m,
            launch: Some(PopupZoneLaunch {
                exit_speed_ms: 30.0,
                vla_deg: 60.0,
                hla_deg: 25.0,
                foul,
                projected_infield_bounce: true,
                projected_outfield_bounce: false,
                clean_infielder_air_out: false,
                foul_infielder_air_out: false,
            }),
            ..live
        };
        let mut swing = PopupZoneSwing {
            count_seq: 1,
            intent: BattedBallIntent::LineDrive,
            baseline: live,
            grid: vec![popup(-0.24, 0.02, true)],
            fixed_horizontal: None,
        };
        assert!(nearest_popup(&swing).is_none());
        swing
            .grid
            .extend([popup(-0.15, 0.02, false), popup(-0.20, 0.05, false)]);
        let nearest = nearest_popup(&swing).unwrap();
        assert_eq!(nearest.cell, 2);
        assert!((nearest.dx_mm - 40.0).abs() < 1e-9);
        assert!((nearest.dz_mm - 30.0).abs() < 1e-9);
        assert!((nearest.distance_mm - 50.0).abs() < 1e-9);
    }

    #[test]
    fn bins_live_offsets_with_half_step_boundaries() {
        assert_eq!(nearest_cell(-0.181, &POPUP_SCAN_X_MM), None);
        assert_eq!(nearest_cell(-0.180, &POPUP_SCAN_X_MM), Some(0));
        assert_eq!(nearest_cell(0.020, &POPUP_SCAN_X_MM), Some(5));
        assert_eq!(nearest_cell(0.180, &POPUP_SCAN_X_MM), None);
        assert_eq!(nearest_cell(0.025, &POPUP_SCAN_Z_MM), Some(0));
        assert_eq!(nearest_cell(0.115, &POPUP_SCAN_Z_MM), None);
    }
}
