//! Observe the live outfield play and compare throws to third without consuming RNG.
use super::{
    game::BattingResult,
    game_state::Ruling,
    player::{Position, RL},
    stadium::Base,
};
use crate::domain::resolver::{
    fielding_physics::{
        CatchType, FieldError, FieldPlayPlayResult, FieldingCatchDiagnostics,
        FieldingSelectionPath, ReachablePickup, pickup_handling, pre_fence_pickup,
    },
    fielding_resolver::{DefensePlayResult, PlayContext, PlayType},
    fielding_timing::DefenseTimeCalculator,
    running_resolver::RunnerAdvanceResult,
};
use crate::domain::util::{PolarPosition, calculate_polar_distance};
use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Default, Serialize)]
struct Margin {
    observations: u64,
    batter_arrives_first: u64,
    mean_defense_minus_runner_sec: f64,
}

impl Margin {
    fn add(&mut self, defense: f64, runner: f64) {
        let gap = defense - runner;
        self.observations += 1;
        self.batter_arrives_first += u64::from(gap > 0.0);
        self.mean_defense_minus_runner_sec +=
            (gap - self.mean_defense_minus_runner_sec) / self.observations as f64;
    }
}

#[derive(Default, Serialize)]
struct Routes {
    live_clock_vs_second: Margin,
    live_clock_vs_third: Margin,
    direct_throw_to_third: Margin,
    observed_return_to_second_then_throw_to_third: Margin,
    from_catch_via_second_to_third: Margin,
    fastest_tested_route_to_third: Margin,
    third_opportunity_only_with_tested_routes: u64,
    third_opportunity_only_with_live_clock: u64,
}

#[derive(Serialize)]
struct PickupPhases {
    search_start_sec: f64,
    pickup_start_sec: f64,
    modeled_travel_sec: f64,
    wait_at_pickup_sec: f64,
    handling_sec: f64,
    stopped_before_pickup_sec: f64,
    safety_fallback: bool,
}

#[derive(Serialize)]
struct PreFenceSearch {
    alternative_search_start_sec: f64,
    live_search_start_sec: f64,
    live_pickup_start_sec: f64,
    first_reachable: Option<ReachablePickup>,
    pickup_start_gain_sec: Option<f64>,
    modeled_recovery_sec: Option<f64>,
    recovery_gain_sec: Option<f64>,
}

#[derive(Serialize)]
struct Recovery {
    fielder_id: i64,
    fielder_position: Position,
    fielder_start: PolarPosition,
    fielder_prep_sec: f64,
    fielder_speed_ms: f64,
    selection_path: FieldingSelectionPath,
    catch_type: CatchType,
    error: Option<FieldError>,
    recovery_position: PolarPosition,
    fielder_displacement_m: f64,
    first_bounce_sec: Option<f64>,
    first_bounce_position: Option<PolarPosition>,
    fence_impact_sec: Option<f64>,
    fence_impact_position: Option<PolarPosition>,
    unfielded_ball_stop_sec: f64,
    unfielded_ball_stop_position: PolarPosition,
    fence_to_recovery_sec: Option<f64>,
    fence_to_recovery_displacement_m: Option<f64>,
    pickup: Option<PickupPhases>,
    pre_fence_search: Option<PreFenceSearch>,
}

impl Recovery {
    fn new(
        ctx: &PlayContext<'_>,
        fielding: FieldingCatchDiagnostics,
        control_start_sec: f64,
        earliest_control_sec: f64,
    ) -> Self {
        let ball = &ctx.fielded_ball.ball;
        let fielder = ctx.try_catch_fielder;
        let recovery_position = ctx.fielded_ball.catch_position;
        let displacement = calculate_polar_distance(&fielder.polar_position, &recovery_position);
        let fence_impact_sec = ball.fence_impact_time.filter(|&t| t <= control_start_sec);
        let fence_impact_position = fence_impact_sec.and(ball.fence_impact_position);
        let first_bounce_sec = ball.first_bounce_time.filter(|&t| t <= control_start_sec);
        let pickup = fielding.pickup.map(|p| PickupPhases {
            search_start_sec: p.search_start_sec,
            pickup_start_sec: p.pickup_time_sec,
            modeled_travel_sec: calculate_polar_distance(&p.initial_position, &p.pickup_position)
                / p.running_speed_ms,
            wait_at_pickup_sec: fielding.waiting_time_sec,
            handling_sec: p.handling_delay_sec,
            stopped_before_pickup_sec: (p.pickup_time_sec - ball.total_time).max(0.0),
            safety_fallback: p.safety_fallback,
        });
        let pre_fence_search = fielding.pickup.and_then(|live| {
            let bounce = first_bounce_sec?;
            let fence = fence_impact_sec?;
            if bounce >= fence || live.search_start_sec != fence {
                return None;
            }
            let first_reachable = pre_fence_pickup(ball, fielder, earliest_control_sec);
            let modeled_recovery_sec =
                first_reachable
                    .zip(live.handling_random)
                    .map(|(point, draw)| {
                        point.time_sec + pickup_handling(live.catching, false, draw).0
                    });
            Some(PreFenceSearch {
                alternative_search_start_sec: bounce,
                live_search_start_sec: live.search_start_sec,
                live_pickup_start_sec: live.pickup_time_sec,
                first_reachable,
                pickup_start_gain_sec: first_reachable
                    .map(|point| live.pickup_time_sec - point.time_sec),
                modeled_recovery_sec,
                recovery_gain_sec: modeled_recovery_sec.map(|time| fielding.catch_time_sec - time),
            })
        });
        Self {
            fielder_id: fielder.id,
            fielder_position: fielder.position,
            fielder_start: fielder.polar_position,
            fielder_prep_sec: fielder.info.prep_time,
            fielder_speed_ms: fielder.info.running_speed,
            selection_path: fielding.selection_path,
            catch_type: fielding.catch_type,
            error: fielding.error_type,
            recovery_position,
            fielder_displacement_m: displacement,
            first_bounce_sec,
            first_bounce_position: first_bounce_sec.and(ball.first_bounce_position),
            fence_impact_sec,
            fence_impact_position,
            unfielded_ball_stop_sec: ball.total_time,
            unfielded_ball_stop_position: ball.final_position,
            fence_to_recovery_sec: fence_impact_sec.map(|t| fielding.catch_time_sec - t),
            fence_to_recovery_displacement_m: fence_impact_position
                .map(|p| calculate_polar_distance(&p, &recovery_position)),
            pickup,
            pre_fence_search,
        }
    }
}

#[derive(Serialize)]
struct DeepOutfieldPlay {
    game_id: u32,
    count_seq: u16,
    location: &'static str,
    result: BattingResult,
    live_throw_target: Base,
    fielding_sec: f64,
    live_defense_sec: f64,
    direct_to_third_after_fielding_sec: f64,
    via_second_after_fielding_sec: Option<f64>,
    batter_to_third_sec: f64,
    recovery: Recovery,
}

#[derive(Serialize)]
struct RunningOutcome {
    result: BattingResult,
    ruling: Ruling,
    targeted_runner_id: Option<i64>,
    runs_scored: u8,
}

impl From<&RunnerAdvanceResult> for RunningOutcome {
    fn from(value: &RunnerAdvanceResult) -> Self {
        Self {
            result: value.batting_result,
            ruling: value.ruling,
            targeted_runner_id: value.target_runner.map(|runner| runner.id),
            runs_scored: value.runs_scored,
        }
    }
}

#[derive(Serialize)]
struct BaseRace {
    ball_arrival_sec: f64,
    ball_minus_batter_sec: f64,
}

impl BaseRace {
    fn new(ball_arrival_sec: f64, batter_arrival_sec: f64) -> Self {
        Self {
            ball_arrival_sec,
            ball_minus_batter_sec: ball_arrival_sec - batter_arrival_sec,
        }
    }
}

#[derive(Serialize)]
struct PickupReplay {
    pickup_position: PolarPosition,
    ready_to_throw_sec: f64,
    fixed_target_defense_sec: f64,
    direct_to_second: BaseRace,
    direct_to_third: BaseRace,
    via_second_to_third: Option<BaseRace>,
    running: Option<RunningOutcome>,
}

struct FixedPlay<'a, 'b> {
    ctx: &'a PlayContext<'b>,
    defense: &'a DefensePlayResult,
    batting_side: RL,
    batter_to_second_sec: f64,
    batter_to_third_sec: f64,
}

impl FixedPlay<'_, '_> {
    fn replay(&self, pickup_position: PolarPosition, ready_to_throw_sec: f64) -> PickupReplay {
        let clock = DefenseTimeCalculator::default();
        let fielder = self.ctx.try_catch_fielder;
        let cutoff = self
            .defense
            .cutoff_fielder_id
            .and_then(|id| self.ctx.fielders.iter().find(|f| f.id == id));
        let target = self.defense.throw_target_base;
        let defense_time = ready_to_throw_sec
            + clock.best_outfield_throw_time(
                fielder,
                &pickup_position,
                target,
                self.defense.play_type != PlayType::ForcePlay,
                cutoff,
            );
        let defense = DefensePlayResult {
            time_to_field: ready_to_throw_sec,
            defense_time,
            ..self.defense.clone()
        };
        let running = self
            .ctx
            .runners
            .after_outfield_hit(&defense, self.batting_side);
        let direct_to = |base| {
            ready_to_throw_sec + clock.direct_play_time(fielder, &pickup_position, base, true)
        };
        let via_second_to_third = if target == Base::Second {
            self.ctx
                .fielders
                .iter()
                .find(|f| f.id == self.defense.final_fielder_id)
                .map(|receiver| {
                    let return_to_second = clock.best_outfield_throw_time(
                        fielder,
                        &pickup_position,
                        Base::Second,
                        false,
                        cutoff,
                    );
                    let second_to_third = clock.direct_play_time(
                        receiver,
                        &Base::Second.polar_position(),
                        Base::Third,
                        true,
                    );
                    BaseRace::new(
                        ready_to_throw_sec + return_to_second + second_to_third,
                        self.batter_to_third_sec,
                    )
                })
        } else {
            None
        };
        PickupReplay {
            pickup_position,
            ready_to_throw_sec,
            fixed_target_defense_sec: defense_time,
            direct_to_second: BaseRace::new(direct_to(Base::Second), self.batter_to_second_sec),
            direct_to_third: BaseRace::new(direct_to(Base::Third), self.batter_to_third_sec),
            via_second_to_third,
            running: running.ok().as_ref().map(RunningOutcome::from),
        }
    }
}

#[derive(Serialize)]
struct FixedPickupComparison {
    game_id: u32,
    count_seq: u16,
    fielder_id: i64,
    runner_base_mask: u8,
    batter_to_second_sec: f64,
    batter_to_third_sec: f64,
    live_throw_target: Base,
    live_defense_sec: f64,
    live_running: RunningOutcome,
    actual_pickup: PickupReplay,
    pre_fence: Option<PickupReplay>,
}

const THIRD_BASE_ADVANCE_MARGINS_SEC: [f64; 3] = [0.0, 0.1, 0.2];

fn advance_with_margin(batter_lead_sec: f64, required_lead_sec: f64) -> bool {
    batter_lead_sec > required_lead_sec
}

#[derive(Serialize)]
struct ThirdBaseAdvanceCandidate {
    game_id: u32,
    count_seq: u16,
    location: &'static str,
    runner_base_mask: u8,
    batter_to_second_sec: f64,
    batter_to_third_sec: f64,
    observed_return_to_second_sec: f64,
    direct_to_third_sec: f64,
    via_second_to_third_sec: Option<f64>,
    batter_lead_at_third_sec: f64,
}

#[derive(Serialize)]
struct ThirdBaseAdvanceAudit {
    observed_doubles: u64,
    doubles_planned_to_second_with_return_to_second: u64,
    required_batter_leads_sec: [f64; 3],
    would_attempt_third: [u64; 3],
    candidates: Vec<ThirdBaseAdvanceCandidate>,
}

impl Default for ThirdBaseAdvanceAudit {
    fn default() -> Self {
        Self {
            observed_doubles: 0,
            doubles_planned_to_second_with_return_to_second: 0,
            required_batter_leads_sec: THIRD_BASE_ADVANCE_MARGINS_SEC,
            would_attempt_third: [0; 3],
            candidates: Vec::new(),
        }
    }
}

impl ThirdBaseAdvanceAudit {
    fn observe(&mut self, candidate: ThirdBaseAdvanceCandidate) {
        self.doubles_planned_to_second_with_return_to_second += 1;
        for (required, count) in self
            .required_batter_leads_sec
            .iter()
            .zip(&mut self.would_attempt_third)
        {
            *count += u64::from(advance_with_margin(
                candidate.batter_lead_at_third_sec,
                *required,
            ));
        }
        if advance_with_margin(candidate.batter_lead_at_third_sec, 0.0) {
            self.candidates.push(candidate);
        }
    }
}

#[derive(Default, Serialize)]
pub struct OutfieldAdvanceAudit {
    by_location_and_result: BTreeMap<&'static str, BTreeMap<String, BTreeMap<String, Routes>>>,
    unavailable_runner_times: u64,
    deep_extra_base_plays: Vec<DeepOutfieldPlay>,
    deep_single_plays: Vec<DeepOutfieldPlay>,
    fixed_pickup_comparisons: Vec<FixedPickupComparison>,
    third_base_advance: ThirdBaseAdvanceAudit,
}

thread_local! {
    static ACTIVE: RefCell<Option<OutfieldAdvanceAudit>> = const { RefCell::new(None) };
}

impl OutfieldAdvanceAudit {
    pub fn observe_game<T>(&mut self, run: impl FnOnce() -> T) -> T {
        struct Restore<'a> {
            previous: Option<OutfieldAdvanceAudit>,
            target: &'a mut OutfieldAdvanceAudit,
        }
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                *self.target = ACTIVE.replace(self.previous.take()).unwrap();
            }
        }
        let previous = ACTIVE.replace(Some(std::mem::take(self)));
        let _restore = Restore {
            previous,
            target: self,
        };
        run()
    }
}

pub(crate) fn record(
    ctx: &PlayContext<'_>,
    game_id: u32,
    count_seq: u16,
    batting_side: RL,
    defense: &DefensePlayResult,
    field_play: &FieldPlayPlayResult,
    running: &RunnerAdvanceResult,
) {
    // The optional right-field force at first has a different advance model.
    if !ctx.fielded_ball.fielded_by.is_outfielder() || defense.throw_target_base == Base::First {
        return;
    }
    ACTIVE.with_borrow_mut(|active| {
        let Some(audit) = active else { return };
        let times = (
            ctx.runners
                .batter_runner_time_to(Base::Second, true, batting_side),
            ctx.runners
                .batter_runner_time_to(Base::Third, true, batting_side),
        );
        let (Ok(second), Ok(third)) = times else {
            audit.unavailable_runner_times += 1;
            return;
        };
        let fielding = field_play.diagnostics();
        let result = running.batting_result;
        let ball = &ctx.fielded_ball.ball;
        let field_time = defense.time_to_field;
        let control_start_sec = fielding
            .pickup
            .map_or(fielding.catch_time_sec, |pickup| pickup.pickup_time_sec);
        let location = if ball
            .fence_impact_time
            .is_some_and(|time| time <= control_start_sec)
        {
            "wall_impact"
        } else if ball
            .first_bounce_time
            .is_some_and(|time| time <= control_start_sec)
            && ball
                .first_bounce_position
                .is_some_and(|p| p.distance >= 75.0 && (8.0..18.0).contains(&p.angle.abs()))
        {
            "deep_gap_first_bounce"
        } else {
            "other_outfield"
        };
        let routes = audit
            .by_location_and_result
            .entry(location)
            .or_default()
            .entry(result.as_ref().into())
            .or_default()
            .entry(defense.throw_target_base.as_ref().into())
            .or_default();
        routes
            .live_clock_vs_second
            .add(defense.defense_time, second);
        routes.live_clock_vs_third.add(defense.defense_time, third);

        let clock = DefenseTimeCalculator::default();
        let direct_throw_sec = clock.direct_play_time(
            ctx.try_catch_fielder,
            &ctx.fielded_ball.catch_position,
            Base::Third,
            true,
        );
        let direct = field_time + direct_throw_sec;
        routes.direct_throw_to_third.add(direct, third);
        let mut fastest = direct;
        let mut via_second_throw_sec = None;
        if defense.throw_target_base == Base::Second {
            if let Some(receiver) = ctx
                .fielders
                .iter()
                .find(|f| f.id == defense.final_fielder_id)
            {
                let second_to_third = clock.direct_play_time(
                    receiver,
                    &Base::Second.polar_position(),
                    Base::Third,
                    true,
                );
                routes
                    .observed_return_to_second_then_throw_to_third
                    .add(defense.defense_time + second_to_third, third);
                let cutoff = defense
                    .cutoff_fielder_id
                    .and_then(|id| ctx.fielders.iter().find(|f| f.id == id));
                let from_catch_to_second = clock.best_outfield_throw_time(
                    ctx.try_catch_fielder,
                    &ctx.fielded_ball.catch_position,
                    Base::Second,
                    false,
                    cutoff,
                );
                let throw_sec = from_catch_to_second + second_to_third;
                via_second_throw_sec = Some(throw_sec);
                let via_second = field_time + throw_sec;
                routes.from_catch_via_second_to_third.add(via_second, third);
                fastest = fastest.min(via_second);
            }
        }
        routes.fastest_tested_route_to_third.add(fastest, third);
        let live_first = defense.defense_time > third;
        let tested_first = fastest > third;
        routes.third_opportunity_only_with_tested_routes += u64::from(tested_first && !live_first);
        routes.third_opportunity_only_with_live_clock += u64::from(live_first && !tested_first);
        if location != "other_outfield" && result == BattingResult::Double {
            let advance = &mut audit.third_base_advance;
            advance.observed_doubles += 1;
            // The live planner tries for second when its clock is in (second, third].
            if defense.throw_target_base == Base::Second
                && defense.defense_time > second
                && !live_first
            {
                advance.observe(ThirdBaseAdvanceCandidate {
                    game_id,
                    count_seq,
                    location,
                    runner_base_mask: ctx.runners.base_mask(),
                    batter_to_second_sec: second,
                    batter_to_third_sec: third,
                    observed_return_to_second_sec: defense.defense_time,
                    direct_to_third_sec: direct,
                    via_second_to_third_sec: via_second_throw_sec.map(|throw| field_time + throw),
                    batter_lead_at_third_sec: fastest - third,
                });
            }
        }
        let deep_hit = location != "other_outfield"
            && matches!(
                result,
                BattingResult::Single | BattingResult::Double | BattingResult::Triple
            );
        if !deep_hit && result != BattingResult::Triple && location != "wall_impact" {
            return;
        }
        let earliest_control_sec = field_play
            .covering_interception
            .as_ref()
            .map_or(0.0, |_| field_play.primary_interception.catch_time_sec);
        let recovery = Recovery::new(ctx, fielding, control_start_sec, earliest_control_sec);
        let early = recovery
            .pre_fence_search
            .as_ref()
            .and_then(|p| p.first_reachable);
        if early.is_some() || result == BattingResult::Triple {
            let fixed = FixedPlay {
                ctx,
                defense,
                batting_side,
                batter_to_second_sec: second,
                batter_to_third_sec: third,
            };
            let pre_fence = early
                .zip(
                    recovery
                        .pre_fence_search
                        .as_ref()
                        .and_then(|p| p.modeled_recovery_sec),
                )
                .map(|(point, ready)| fixed.replay(point.position, ready));
            audit.fixed_pickup_comparisons.push(FixedPickupComparison {
                game_id,
                count_seq,
                fielder_id: ctx.try_catch_fielder.id,
                runner_base_mask: ctx.runners.base_mask(),
                batter_to_second_sec: second,
                batter_to_third_sec: third,
                live_throw_target: defense.throw_target_base,
                live_defense_sec: defense.defense_time,
                live_running: RunningOutcome::from(running),
                actual_pickup: fixed.replay(ctx.fielded_ball.catch_position, field_time),
                pre_fence,
            });
        }
        if deep_hit {
            let plays = if result == BattingResult::Single {
                &mut audit.deep_single_plays
            } else {
                &mut audit.deep_extra_base_plays
            };
            plays.push(DeepOutfieldPlay {
                game_id,
                count_seq,
                location,
                result,
                live_throw_target: defense.throw_target_base,
                fielding_sec: field_time,
                live_defense_sec: defense.defense_time,
                direct_to_third_after_fielding_sec: direct_throw_sec,
                via_second_after_fielding_sec: via_second_throw_sec,
                batter_to_third_sec: third,
                recovery,
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn third_base_advance_requires_a_strict_lead_over_each_threshold() {
        let mut audit = ThirdBaseAdvanceAudit::default();
        for lead in [-0.05, 0.0, 0.05, 0.10, 0.15, 0.20, 0.21] {
            audit.observe(ThirdBaseAdvanceCandidate {
                game_id: 1,
                count_seq: 1,
                location: "wall_impact",
                runner_base_mask: 0,
                batter_to_second_sec: 8.0,
                batter_to_third_sec: 11.0,
                observed_return_to_second_sec: 9.0,
                direct_to_third_sec: 11.0 + lead,
                via_second_to_third_sec: None,
                batter_lead_at_third_sec: lead,
            });
        }
        assert_eq!(audit.doubles_planned_to_second_with_return_to_second, 7);
        assert_eq!(audit.would_attempt_third, [5, 3, 1]);
        assert_eq!(audit.candidates.len(), 5);
    }
}
