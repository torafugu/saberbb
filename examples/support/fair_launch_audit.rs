//! Launch parameters paired with official fair-ball results, regardless of fielding path.
use super::bip_audit::{CONVERSION_EV_EDGES_MS, CONVERSION_HLA_EDGES_DEG, band, is_bip};
use saberbb::domain::{
    resolver::fielding_physics::CatchType,
    shared::game::{BattingResult, GameResult},
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

const HIGH_EV_MIN_MS: f64 = 45.0;
const TARGET_VLA: std::ops::Range<f64> = 7.0..10.0;

#[derive(Default, Serialize)]
struct Outcomes {
    plays: u64,
    by_result: BTreeMap<String, u64>,
    direct_catch_outs: u64,
}

impl Outcomes {
    fn observe(&mut self, result: BattingResult, direct_catch: bool) {
        self.plays += 1;
        *self.by_result.entry(result.as_ref().into()).or_default() += 1;
        self.direct_catch_outs += u64::from(direct_catch && result == BattingResult::Out);
    }
}

#[derive(Default, Serialize)]
pub(super) struct FairLaunchAudit {
    pub(super) home_runs: u64,
    pub(super) non_hr_in_play: u64,
    missing_exit_speed: u64,
    ev_ge_45_ms: u64,
    ev_ge_45_ms_missing_vla: u64,
    ev_ge_45_ms_by_vla: BTreeMap<String, Outcomes>,
    ev_ge_45_ms_vla_7_to_10: Outcomes,
    ev_ge_45_ms_vla_7_to_10_by_ev_vla_hla: BTreeMap<String, Outcomes>,
}

impl FairLaunchAudit {
    pub(super) fn observe(&mut self, game: &GameResult) {
        let direct_catches: BTreeSet<_> = game
            .player_fieldings
            .iter()
            .filter(|f| {
                f.seq == 1
                    && f.catch_diagnostics
                        .is_some_and(|catch| catch.catch_type == CatchType::DirectFly)
            })
            .map(|f| f.count_seq)
            .collect();
        for b in &game.player_battings {
            // The stored ball carries the launch parameters even on the early-return
            // paths for direct catches, home runs, and ground-rule doubles.
            if b.result == BattingResult::HomeRun {
                self.home_runs += 1;
            } else if is_bip(b.result) {
                let hla = (b.hla_model_version == Some(2)).then_some(b.final_hla_deg);
                self.observe_in_play(
                    b.ball.launch_speed,
                    b.ball.launch_angle,
                    hla,
                    b.result,
                    direct_catches.contains(&b.count_seq),
                );
            }
        }
    }

    fn observe_in_play(
        &mut self,
        ev: f64,
        vla: f64,
        hla: Option<f64>,
        result: BattingResult,
        direct_catch: bool,
    ) {
        self.non_hr_in_play += 1;
        if !ev.is_finite() || ev <= 0.0 {
            self.missing_exit_speed += 1;
            return;
        }
        if ev < HIGH_EV_MIN_MS {
            return;
        }
        self.ev_ge_45_ms += 1;
        if !vla.is_finite() || !(-90.0..=90.0).contains(&vla) {
            self.ev_ge_45_ms_missing_vla += 1;
            return;
        }

        let angle = band(
            vla,
            &[-10.0, 0.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 25.0, 50.0],
        );
        self.ev_ge_45_ms_by_vla
            .entry(angle.clone())
            .or_default()
            .observe(result, direct_catch);
        if TARGET_VLA.contains(&vla) {
            self.ev_ge_45_ms_vla_7_to_10.observe(result, direct_catch);
            let speed = band(ev, CONVERSION_EV_EDGES_MS);
            let direction = band(hla.unwrap_or(f64::NAN), CONVERSION_HLA_EDGES_DEG);
            self.ev_ge_45_ms_vla_7_to_10_by_ev_vla_hla
                .entry(format!("EV:{speed}|VLA:{angle}|HLA:{direction}"))
                .or_default()
                .observe(result, direct_catch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_direct_catch_outs_and_uses_half_open_launch_bands() {
        let mut audit = FairLaunchAudit::default();
        audit.observe_in_play(44.99, 7.0, Some(0.0), BattingResult::Out, false);
        audit.observe_in_play(45.0, 7.0, Some(0.0), BattingResult::Out, true);
        audit.observe_in_play(45.0, 9.99, None, BattingResult::Single, false);
        audit.observe_in_play(45.0, 10.0, Some(0.0), BattingResult::Double, false);
        audit.observe_in_play(45.0, f64::NAN, None, BattingResult::Out, false);

        assert_eq!(audit.non_hr_in_play, 5);
        assert_eq!(audit.ev_ge_45_ms, 4);
        assert_eq!(audit.ev_ge_45_ms_missing_vla, 1);
        let target = &audit.ev_ge_45_ms_vla_7_to_10;
        assert_eq!(target.plays, 2);
        assert_eq!(target.by_result["Single"], 1);
        assert_eq!(target.direct_catch_outs, 1);
        assert_eq!(
            audit.ev_ge_45_ms_vla_7_to_10_by_ev_vla_hla["EV:[45,50)|VLA:[7,8)|HLA:[-5,5)"]
                .direct_catch_outs,
            1
        );
    }
}
