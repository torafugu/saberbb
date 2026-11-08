//! Same-target throw timing sensitivity, using only baseline-reproducing records.
use super::pickup_timing_audit::PickupTimingAudit;
use saberbb::domain::{
    resolver::{
        fielding_physics::{CatchType, estimate_ball_state_at_time},
        fielding_resolver::PlayType,
        fielding_timing::DefenseTimeCalculator,
        running_resolver::RunningEvent,
    },
    shared::{
        game::{BattingResult, GameResult, GameSchedule},
        game_state::ActiveFielder,
    },
    util::PolarPosition,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Default, Serialize)]
struct Moments {
    n: u64,
    mean: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
}
impl Moments {
    fn add(&mut self, v: f64) {
        self.n += 1;
        self.mean = Some(self.mean.unwrap_or(v) + (v - self.mean.unwrap_or(v)) / self.n as f64);
        self.min = Some(self.min.map_or(v, |x| x.min(v)));
        self.max = Some(self.max.map_or(v, |x| x.max(v)));
    }
}
#[derive(Default, Serialize)]
struct ThrowComparison {
    eligible: u64,
    missing_inputs: u64,
    baseline_mismatch: u64,
    baseline_matched: u64,
    target_comparable: u64,
    safe_to_out: u64,
    out_to_safe: u64,
    batter_safe_to_out: u64,
    batter_out_to_safe: u64,
    corrected_minus_recorded_sec: Moments,
}
impl ThrowComparison {
    fn compare(
        &mut self,
        baseline: f64,
        corrected: f64,
        recorded: f64,
        runner: f64,
        target: Option<i64>,
        batter: i64,
    ) {
        if !baseline.is_finite()
            || !corrected.is_finite()
            || !recorded.is_finite()
            || (baseline - recorded).abs() >= 1e-7
        {
            self.baseline_mismatch += 1;
            return;
        }
        self.baseline_matched += 1;
        self.corrected_minus_recorded_sec.add(corrected - recorded);
        if target.is_some() && runner.is_finite() && runner > 0.0 && recorded >= 0.0 {
            self.target_comparable += 1;
            let was_safe = recorded >= runner;
            let now_safe = corrected >= runner;
            self.safe_to_out += u64::from(was_safe && !now_safe);
            self.out_to_safe += u64::from(!was_safe && now_safe);
            if target == Some(batter) {
                self.batter_safe_to_out += u64::from(was_safe && !now_safe);
                self.batter_out_to_safe += u64::from(!was_safe && now_safe);
            }
        }
    }
}
#[derive(Default, Serialize)]
struct FinalPickupComparison {
    timing: PickupTimingAudit,
    throw_origin: ThrowComparison,
    safety_fallback_throw_origin: ThrowComparison,
    no_grounder_play_record: u64,
    timing_not_reproduced: u64,
    examples: Vec<Value>,
}
#[derive(Default, Serialize)]
pub struct ThrowOriginAudit {
    #[serde(flatten)]
    ordinary: ThrowComparison,
    final_pickup: FinalPickupComparison,
}
impl ThrowOriginAudit {
    pub fn observe(&mut self, schedule: &GameSchedule, game: &GameResult) {
        let roster: BTreeMap<_, _> = schedule
            .away_team
            .players
            .iter()
            .chain(&schedule.home_team.players)
            .map(|p| (p.info.id, p))
            .collect();
        for b in &game.player_battings {
            if !matches!(
                b.result,
                BattingResult::Single
                    | BattingResult::Double
                    | BattingResult::Triple
                    | BattingResult::Out
                    | BattingResult::FieldersChoice
                    | BattingResult::DoublePlay
            ) {
                continue;
            }
            let fields: Vec<_> = game
                .player_fieldings
                .iter()
                .filter(|f| f.count_seq == b.count_seq && f.seq == 1)
                .collect();
            if fields.len() != 1 {
                continue;
            }
            let f = fields[0];
            let Some(diag) = f.catch_diagnostics else {
                continue;
            };
            let is_pickup = diag.catch_type == CatchType::FinalPickup;
            if !is_pickup
                && (diag.error_type.is_some()
                    || !matches!(
                        diag.catch_type,
                        CatchType::DirectFly | CatchType::BounceCatch
                    ))
            {
                continue;
            }
            let selected: Vec<_> = game
                .player_fielding_candidates
                .iter()
                .filter(|c| {
                    c.count_seq == b.count_seq
                        && c.diagnostics.selected
                        && c.diagnostics.fielder_position == f.catch_fielder_position
                })
                .collect();
            let selected_id = (selected.len() == 1).then(|| selected[0].diagnostics.fielder_id);
            let pickup_origin = if is_pickup {
                let origin = self.final_pickup.timing.record(
                    diag,
                    &b.ball,
                    f.time_to_field,
                    selected_id,
                    game.id,
                    b.count_seq,
                    b.result.as_ref(),
                );
                if origin.is_none() {
                    self.final_pickup.timing_not_reproduced += 1;
                    continue;
                }
                origin
            } else {
                None
            };
            let Some(r) = game
                .player_runnings
                .iter()
                .filter(|r| r.count_seq == b.count_seq && r.event == RunningEvent::GrounderPlay)
                .min_by_key(|r| r.seq)
            else {
                if is_pickup {
                    self.final_pickup.no_grounder_play_record += 1;
                }
                continue;
            };
            let audit = if is_pickup && diag.pickup.is_some_and(|p| p.safety_fallback) {
                &mut self.final_pickup.safety_fallback_throw_origin
            } else if is_pickup {
                &mut self.final_pickup.throw_origin
            } else {
                &mut self.ordinary
            };
            audit.eligible += 1;
            let inputs = (|| {
                // Legacy catch_fielder_id actually stores the receiver ID on these plays.
                // Selected candidate diagnostics retain the true initial thrower identity.
                let id = selected_id?;
                let thrower = ActiveFielder::new(
                    f.catch_fielder_position,
                    id,
                    roster.get(&id)?.fielder().ok()?,
                );
                let cutoff = match f.cutoff_fielder_id {
                    Some(id) => Some(ActiveFielder::new(
                        f.cutoff_fielder_position?,
                        id,
                        roster.get(&id)?.fielder().ok()?,
                    )),
                    None => None,
                };
                Some((thrower, cutoff, f.final_fielder_position?))
            })();
            let Some((thrower, cutoff, receiver)) = inputs else {
                audit.missing_inputs += 1;
                continue;
            };
            let calc = DefenseTimeCalculator::default();
            let phase = |origin: &PolarPosition| {
                if b.ball.is_infield() {
                    calc.infield_play_time(
                        &thrower,
                        origin,
                        r.throw_target_base,
                        f.play_type == PlayType::ForcePlay,
                        receiver,
                    )
                    .defense_time
                } else {
                    calc.best_outfield_throw_time(
                        &thrower,
                        origin,
                        r.throw_target_base,
                        f.play_type != PlayType::ForcePlay,
                        cutoff.as_ref(),
                    )
                }
            };
            let baseline = f.time_to_field + phase(&b.ball.final_position);
            let (distance, angle, _, _, _, _) =
                estimate_ball_state_at_time(diag.catch_time_sec, &b.ball);
            let origin = pickup_origin.unwrap_or_else(|| PolarPosition::new(distance, angle));
            let corrected = f.time_to_field + phase(&origin);
            let flips_before = audit.safe_to_out + audit.out_to_safe;
            audit.compare(
                baseline,
                corrected,
                r.defense_time,
                r.runner_time,
                r.target_runner_id,
                b.batter_id,
            );
            if is_pickup
                && audit.safe_to_out + audit.out_to_safe > flips_before
                && self.final_pickup.examples.len() < 4
            {
                self.final_pickup.examples.push(json!({"game_id":game.id,"count_seq":b.count_seq,"batter_id":b.batter_id,
                    "target_runner_id":r.target_runner_id,"runner_time_sec":r.runner_time,"recorded_defense_time_sec":r.defense_time,
                    "baseline_sec":baseline,"corrected_sec":corrected,"throw_origin":origin,"diagnostics":diag}));
            }
        }
    }
    pub fn json(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap();
        v["final_pickup"]["source"] = json!("final_pickup_timing_v1");
        v["definitions"] = json!([
            "Top-level counters retain the error-free DirectFly/BounceCatch cohort. final_pickup separately reconstructs fresh pickup search, position, handling and completion clocks (including captured fumbles and safety fallbacks); it never treats completion-time position as pickup position. Missing old snapshots remain missing. Safety fallback clocks reproduce the existing heuristic, not physical arrival; required-arrival deficits are reported and their throw comparisons are separate.",
            "The unique selected candidate identifies the thrower; legacy catch_fielder_id holds the receiver ID. Source-roster fielder skills and recorded cutoff/receiver are used; original defense time must reproduce within 1e-7 s.",
            "Change only the origin to the model catch position, using the live timing calculator (direct/relay/run-to-base options); keep target/runner time/fielding time fixed. Missing or nonpositive runner times cannot establish a ruling.",
            "Safe/out flips concern that targeted runner, possibly a lead runner. They do not predict hits, runs, alternate target choices, subsequent throws or full-game outcomes. baseline_matched + baseline_mismatch + missing_inputs = eligible within each throw cohort. FinalPickup clock/search/handling mismatches are excluded before throw comparison. Its target may be a lead runner; flips are not predicted hit conversions."
        ]);
        v
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reproduction_gate_and_lead_runner_identity_are_preserved() {
        let mut audit = ThrowComparison::default();
        audit.compare(5.0, 4.0, 5.0, 4.5, Some(10), 7);
        audit.compare(4.0, 5.0, 4.0, 4.5, Some(7), 7);
        audit.compare(6.0, 4.0, 5.0, 4.5, Some(7), 7);
        audit.compare(5.0, 4.0, 5.0, 0.0, None, 7);
        audit.compare(f64::NAN, 4.0, 5.0, 4.5, Some(7), 7);
        assert_eq!(
            (
                audit.baseline_matched,
                audit.baseline_mismatch,
                audit.target_comparable
            ),
            (3, 2, 2)
        );
        assert_eq!((audit.safe_to_out, audit.out_to_safe), (1, 1));
        assert_eq!((audit.batter_safe_to_out, audit.batter_out_to_safe), (0, 1));
    }
}
