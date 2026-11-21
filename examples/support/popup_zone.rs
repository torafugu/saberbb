use super::{Args, GameSchedule, derived_seed, simulate_game};
use anyhow::{Context, Result, ensure};
use saberbb::domain::{
    random_provider::RealRng,
    shared::game_state::{
        POPUP_SCAN_INFIELD_M, POPUP_SCAN_VLA_DEG, POPUP_SCAN_X_MM, POPUP_SCAN_Z_MM, PopupZonePlay,
        PopupZoneSwing, with_popup_zone_audit,
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
        if launch.projected_infield_bounce || launch.clean_infielder_air_out {
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
    swings_with_any_grid_popup: u64,
}

impl Default for ZoneCounts {
    fn default() -> Self {
        Self {
            grid: std::array::from_fn(|_| CellCounts::default()),
            baseline: std::array::from_fn(|_| CellCounts::default()),
            baseline_total: CellCounts::default(),
            baseline_outside_grid: CellCounts::default(),
            swings_with_any_grid_popup: 0,
        }
    }
}

impl ZoneCounts {
    fn observe(&mut self, swing: &PopupZoneSwing) {
        self.baseline_total.observe(swing.baseline);
        let x = nearest_cell(swing.baseline.x_m, &POPUP_SCAN_X_MM);
        let z = nearest_cell(swing.baseline.z_m, &POPUP_SCAN_Z_MM);
        if let (Some(x), Some(z)) = (x, z) {
            self.baseline[z * WIDTH + x].observe(swing.baseline);
        } else {
            self.baseline_outside_grid.observe(swing.baseline);
        }
        let mut any_popup = false;
        for (cell, &play) in self.grid.iter_mut().zip(&swing.grid) {
            let before = cell.popups.count;
            cell.observe(play);
            any_popup |= cell.popups.count > before;
        }
        self.swings_with_any_grid_popup += u64::from(any_popup);
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
                })
            })
            .collect();
        json!({
            "live_total": self.baseline_total.report(),
            "live_outside_grid": self.baseline_outside_grid.report(),
            "swings_with_any_grid_popup": self.swings_with_any_grid_popup,
            "cells": cells,
        })
    }
}

pub(super) fn run(args: &Args, games: &[GameSchedule]) -> Result<Value> {
    let mut all = ZoneCounts::default();
    let mut contact_first = ZoneCounts::default();
    let mut line_drive = ZoneCounts::default();
    for &seed in &args.seeds {
        for game in games {
            let (result, swings) = with_popup_zone_audit(|| {
                simulate_game(
                    game.clone(),
                    Box::new(RealRng::from_seed(derived_seed(seed, game.id))),
                    BattingStrategySettings::player_driven(),
                    Some(args.max_pitches),
                    false,
                )
            });
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
                ensure!(swing.grid.len() == CELLS, "popup zone grid size mismatch");
                all.observe(&swing);
                match swing.intent {
                    BattedBallIntent::ContactFirst => &mut contact_first,
                    BattedBallIntent::LineDrive => &mut line_drive,
                }
                .observe(&swing);
            }
            ensure!(live.is_empty(), "unpaired live swings in game {}", game.id);
        }
        eprintln!(
            "Popup contact zone, seed {seed}: {} games complete",
            games.len()
        );
    }
    Ok(json!({
        "definition": "Replace final world X and Z offsets at contact while retaining the live pitch, swing angles, timing, speed and starting RNG state. Positive Z puts the bat below the ball. A popup is a fair launch strictly above the VLA threshold with a projected first bounce in fair territory inside the infield radius or a clean air out by an infielder. Both subcriteria are reported and can overlap. Live offsets are binned into the nearest grid center, with half-step outer boundaries and ties assigned upward. Forced grid cells use the same swings and represent geometric opportunities, not expected outcome frequencies.",
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
    fn bins_live_offsets_with_half_step_boundaries() {
        assert_eq!(nearest_cell(-0.181, &POPUP_SCAN_X_MM), None);
        assert_eq!(nearest_cell(-0.180, &POPUP_SCAN_X_MM), Some(0));
        assert_eq!(nearest_cell(0.020, &POPUP_SCAN_X_MM), Some(5));
        assert_eq!(nearest_cell(0.180, &POPUP_SCAN_X_MM), None);
        assert_eq!(nearest_cell(0.025, &POPUP_SCAN_Z_MM), Some(0));
        assert_eq!(nearest_cell(0.115, &POPUP_SCAN_Z_MM), None);
    }
}
