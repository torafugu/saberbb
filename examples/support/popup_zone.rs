use super::{Args, GameSchedule, derived_seed, simulate_game};
use anyhow::{Context, Result, ensure};
use saberbb::domain::{
    random_provider::RealRng,
    shared::game_state::{
        FixedHorizontalScan, POPUP_FIXED_X_STEP_M, POPUP_SCAN_INFIELD_M, POPUP_SCAN_VLA_DEG,
        POPUP_SCAN_X_MM, POPUP_SCAN_Z_MM, PopupZonePlay, PopupZoneSwing,
        with_fixed_horizontal_popup_audit, with_popup_zone_audit,
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

fn is_popup(play: PopupZonePlay) -> bool {
    play.launch.is_some_and(|launch| {
        launch.vla_deg > POPUP_SCAN_VLA_DEG
            && !launch.foul
            && (launch.projected_infield_bounce || launch.clean_infielder_air_out)
    })
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
        if launch.foul {
            self.high_foul += 1;
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

#[derive(Default)]
struct FixedHorizontalCounts {
    swings: u64,
    already_popup: u64,
    new_popup: u64,
    offset_to_first_popup: Distances,
    limits_and_outcomes: BTreeMap<&'static str, u64>,
    new_popup_launches: LaunchCounts,
}

impl FixedHorizontalCounts {
    fn observe(&mut self, baseline: PopupZonePlay, scan: &FixedHorizontalScan) {
        use FixedHorizontalScan::*;
        self.swings += 1;
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
                            .filter(|launch| launch.vla_deg > POPUP_SCAN_VLA_DEG)
                    };
                    if high().any(|launch| !launch.foul && launch.projected_outfield_bounce) {
                        "high_fair_outfield_bounce_without_popup"
                    } else if high().any(|launch| !launch.foul) {
                        "high_fair_other_without_popup"
                    } else if high().any(|launch| launch.foul) {
                        "high_foul_only"
                    } else {
                        "no_high_contact_at_sampled_positions"
                    }
                }
            }
        };
        *self.limits_and_outcomes.entry(category).or_default() += 1;
    }

    fn report(&self) -> Value {
        json!({
            "swings": self.swings,
            "already_popup": self.already_popup,
            "new_popup": self.new_popup,
            "additional_z_to_first_new_popup": self.offset_to_first_popup.report(self.new_popup),
            "first_new_popup_launches": self.new_popup_launches.report(),
            "limits_and_outcomes": self.limits_and_outcomes,
        })
    }
}

#[derive(Default)]
struct FixedHorizontalGroup {
    all: FixedHorizontalCounts,
    from_live_contact: FixedHorizontalCounts,
    from_live_whiff: FixedHorizontalCounts,
}

impl FixedHorizontalGroup {
    fn observe(&mut self, baseline: PopupZonePlay, scan: &FixedHorizontalScan) {
        self.all.observe(baseline, scan);
        if baseline.launch.is_some() {
            self.from_live_contact.observe(baseline, scan);
        } else {
            self.from_live_whiff.observe(baseline, scan);
        }
    }

    fn report(&self) -> Value {
        json!({
            "all": self.all.report(),
            "from_live_contact": self.from_live_contact.report(),
            "from_live_whiff": self.from_live_whiff.report(),
        })
    }
}

pub(super) fn run(args: &Args, games: &[GameSchedule]) -> Result<Value> {
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
            let (result, swings) = if args.popup_fixed_horizontal {
                with_fixed_horizontal_popup_audit(simulate)
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
                    fixed_all.observe(swing.baseline, scan);
                    match swing.intent {
                        BattedBallIntent::ContactFirst => &mut fixed_contact_first,
                        BattedBallIntent::LineDrive => &mut fixed_line_drive,
                    }
                    .observe(swing.baseline, scan);
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
        return Ok(json!({
            "definition": "Hold each swing's realized world X fixed. Increase Z (ball minus bat) from its live value. Intersect the legal length and thickness interval with the region above the VLA threshold, then replay the intersection at equally spaced points no more than the reported recovery step apart, including endpoints. A popup is a fair launch above the VLA threshold with a projected fair infield first bounce or a clean air out by an infielder. Outfield bounce means a projected fair first bounce at or beyond the infield radius. The outcome groups are exclusive: a new popup takes precedence, followed by high fair outfield bounce, other high fair, then high foul. Additional Z is measured to the first sampled popup; narrow intervals between samples can be missed. This is a geometric replay, independent of a batter's ability to move that far.",
            "vla_threshold_deg": POPUP_SCAN_VLA_DEG,
            "infield_radius_m": POPUP_SCAN_INFIELD_M,
            "maximum_z_sample_step_mm": POPUP_FIXED_X_STEP_M * 1000.0,
            "all": fixed_all.report(),
            "contact_first": fixed_contact_first.report(),
            "line_drive": fixed_line_drive.report(),
        }));
    }
    Ok(json!({
        "definition": "Replace final world X and Z offsets at contact while retaining the live pitch, swing angles, timing, speed and starting RNG state. Positive Z puts the bat below the ball. A popup is a fair launch strictly above the VLA threshold with a projected first bounce in fair territory inside the infield radius or a clean air out by an infielder. Both subcriteria are reported and can overlap. Live offsets are binned into the nearest grid center, with half-step outer boundaries and ties assigned upward. Forced grid cells use the same swings and represent geometric opportunities, not expected outcome frequencies.",
        "nearest_grid_popup_definition": "For each swing, minimize Euclidean distance in world X/Z from the exact ordinary position to a grid replay that produced a fair infield popup. Ties use the first grid cell (increasing Z, then X). Signed displacements equal grid minus live. Include ordinary positions outside the grid and exclude swings with no popup on the scanned grid from distance distributions. Distances are to the tested grid, including when the live swing was already a popup.",
        "vla_threshold_deg": POPUP_SCAN_VLA_DEG,
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
    fn finds_nearest_popup_from_exact_position_and_skips_non_popups() {
        use saberbb::domain::shared::game_state::PopupZoneLaunch;
        let live = PopupZonePlay {
            x_m: -0.24,
            z_m: 0.02,
            launch: None,
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
