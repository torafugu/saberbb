//! Frozen-play checks of infield interception opportunities on outfield pickup singles.
use super::{
    ball::{BattedBall, INFIELD_DISTANCE, TrajectoryType},
    game::BattingResult,
    game_state::ActiveFielder,
    player::{Position, RL},
    stadium::Base,
};
use crate::domain::{
    resolver::{
        fielding_physics::{
            CatchType, FielderCandidateDiagnostics, FieldingCatchDiagnostics,
            estimate_ball_state_at_time,
        },
        fielding_resolver::PlayContext,
        fielding_timing::DefenseTimeCalculator,
    },
    util::{PolarPosition, calculate_polar_distance},
};
use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap};

const SCENARIOS: [&str; 3] = [
    "original_filters_live_grid",
    "without_filters_live_grid",
    "without_filters_10ms",
];

#[derive(Clone, Copy)]
struct Opportunity<'a> {
    fielder: &'a ActiveFielder,
    candidate: &'a FielderCandidateDiagnostics,
    time_sec: f64,
    position: PolarPosition,
    direct: bool,
}

#[derive(Default, Serialize)]
struct Scenario {
    reachable: u64,
    direct_catch: u64,
    bounce: u64,
    first_out: u64,
    first_safe: u64,
    first_unknown: u64,
    by_position: BTreeMap<String, u64>,
    by_live_candidate_reason: BTreeMap<String, u64>,
}

impl Scenario {
    fn add(
        &mut self,
        opportunity: Opportunity<'_>,
        fielders: &[ActiveFielder],
        runner_first: Option<f64>,
    ) {
        self.reachable += 1;
        *self
            .by_position
            .entry(opportunity.fielder.position.as_ref().into())
            .or_default() += 1;
        *self
            .by_live_candidate_reason
            .entry(opportunity.candidate.end_reason().as_ref().into())
            .or_default() += 1;
        if opportunity.direct {
            self.direct_catch += 1;
            return;
        }
        self.bounce += 1;
        match runner_first.zip(first_completion(opportunity, fielders)) {
            Some((runner, defense)) if defense < runner => self.first_out += 1,
            Some(_) => self.first_safe += 1,
            None => self.first_unknown += 1,
        }
    }
}

fn first_completion(opportunity: Opportunity<'_>, fielders: &[ActiveFielder]) -> Option<f64> {
    let fielder = opportunity.fielder;
    let base = Base::First.polar_position();
    let calc = DefenseTimeCalculator::default();
    let carry = opportunity.time_sec + calc.run_to_base_time(fielder, &opportunity.position, &base);
    let receiver = if fielder.position == Position::FB {
        Position::P
    } else {
        Position::FB
    };
    let direct = fielders
        .iter()
        .find(|f| f.position == receiver)
        .and_then(|cover| {
            if !cover.info.prep_time.is_finite()
                || cover.info.prep_time < 0.0
                || !cover.info.running_speed.is_finite()
                || cover.info.running_speed <= 0.0
                || !fielder.info.throw_speed.is_finite()
                || fielder.info.throw_speed <= 0.0
            {
                return None;
            }
            // Cover reacts at the interception. Throw and cover run happen concurrently.
            let cover_duration = cover.info.prep_time
                + calculate_polar_distance(&cover.polar_position, &base) / cover.info.running_speed;
            let throw_duration =
                calc.direct_play_time(fielder, &opportunity.position, Base::First, false);
            let completion = opportunity.time_sec + cover_duration.max(throw_duration);
            completion.is_finite().then_some(completion)
        })?;
    let completion = carry.min(direct);
    completion.is_finite().then_some(completion)
}

fn find_opportunities<'a>(
    ball: &BattedBall,
    candidates: &[(&'a ActiveFielder, &'a FielderCandidateDiagnostics)],
    pickup_sec: f64,
) -> [Option<Opportunity<'a>>; 3] {
    let mut opportunities = [None; 3];
    for index in 0.. {
        // Same 0.10 s start and air sampling as live; the relaxed control
        // also samples every 10 ms after the bounce.
        let t = 0.10 + f64::from(index) * 0.01;
        if t >= pickup_sec || t > ball.total_time {
            break;
        }
        let (distance, angle, x, y, height, direct) = estimate_ball_state_at_time(t, ball);
        if !distance.is_finite() || distance >= INFIELD_DISTANCE {
            break;
        }
        let live_grid = direct || index % 5 == 0;
        for &(fielder, candidate) in candidates {
            let start = fielder.polar_position;
            let dx = x - start.distance * start.angle.to_radians().sin();
            let dy = y - start.distance * start.angle.to_radians().cos();
            let needed = fielder.info.prep_time
                + (dx.powi(2) + dy.powi(2)).sqrt() / fielder.info.running_speed;
            let required_margin = if direct {
                candidate.required_margin_sec
            } else {
                0.0
            };
            if !height.is_finite()
                || height > fielder.info.reach_height
                || !needed.is_finite()
                || needed > t
                || t - needed < required_margin
            {
                continue;
            }
            let opportunity = Opportunity {
                fielder,
                candidate,
                time_sec: t,
                position: PolarPosition::new(distance, angle),
                direct,
            };
            opportunities[2].get_or_insert(opportunity);
            if live_grid {
                opportunities[1].get_or_insert(opportunity);
                if candidate.lane_eligible && candidate.distance_eligible {
                    opportunities[0].get_or_insert(opportunity);
                }
            }
        }
        if opportunities.iter().all(Option::is_some) {
            break;
        }
    }
    opportunities
}

#[derive(Default, Serialize)]
struct Cohort {
    singles: u64,
    missing_inputs: u64,
    by_selection_path: BTreeMap<String, u64>,
    scenarios: BTreeMap<&'static str, Scenario>,
}

impl Cohort {
    fn add(
        &mut self,
        diagnostics: FieldingCatchDiagnostics,
        opportunities: Option<[Option<Opportunity<'_>>; 3]>,
        fielders: &[ActiveFielder],
        runner_first: Option<f64>,
    ) {
        self.singles += 1;
        *self
            .by_selection_path
            .entry(diagnostics.selection_path.as_ref().into())
            .or_default() += 1;
        let Some(opportunities) = opportunities else {
            self.missing_inputs += 1;
            return;
        };
        for (name, opportunity) in SCENARIOS.into_iter().zip(opportunities) {
            let scenario = self.scenarios.entry(name).or_default();
            if let Some(opportunity) = opportunity {
                scenario.add(opportunity, fielders, runner_first);
            }
        }
    }
}

#[derive(Default, Serialize)]
pub struct InfieldOpportunityAudit {
    outfield_pickup_grounder_singles: Cohort,
    hard_low_grounder_subset: Cohort,
}

thread_local! {
    static ACTIVE: RefCell<Option<InfieldOpportunityAudit>> = const { RefCell::new(None) };
}

impl InfieldOpportunityAudit {
    pub fn observe_game<T>(&mut self, run: impl FnOnce() -> T) -> T {
        struct Restore<'a> {
            previous: Option<InfieldOpportunityAudit>,
            target: &'a mut InfieldOpportunityAudit,
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
    batting_side: RL,
    diagnostics: FieldingCatchDiagnostics,
    candidates: &[FielderCandidateDiagnostics],
    result: BattingResult,
) {
    ACTIVE.with_borrow_mut(|active| {
        let Some(audit) = active else { return };
        let ball = &ctx.fielded_ball.ball;
        if result != BattingResult::Single
            || !diagnostics.fielder_position.is_outfielder()
            || diagnostics.catch_type != CatchType::FinalPickup
            || ball.trajectory() != TrajectoryType::Grounder
        {
            return;
        }
        let runner_first = ctx
            .runners
            .batter_runner_time_to(Base::First, true, batting_side)
            .ok()
            .filter(|t| t.is_finite() && *t > 0.0);
        let opportunities = (|| {
            let pickup = diagnostics.pickup?;
            if !pickup.pickup_time_sec.is_finite()
                || pickup.pickup_time_sec < 0.0
                || !ball.total_time.is_finite()
                || ball.total_time < 0.0
            {
                return None;
            }
            let inputs = ctx
                .fielders
                .iter()
                .filter(|f| f.position.is_infielder())
                .map(|fielder| {
                    if !fielder.info.running_speed.is_finite()
                        || fielder.info.running_speed <= 0.0
                        || !fielder.info.prep_time.is_finite()
                        || fielder.info.prep_time < 0.0
                        || !fielder.info.reach_height.is_finite()
                    {
                        return None;
                    }
                    let candidate = candidates.iter().find(|c| {
                        c.fielder_position == fielder.position && c.fielder_id == fielder.id
                    })?;
                    Some((fielder, candidate))
                })
                .collect::<Option<Vec<_>>>()?;
            if inputs.is_empty() {
                return None;
            }
            Some(find_opportunities(ball, &inputs, pickup.pickup_time_sec))
        })();
        audit.outfield_pickup_grounder_singles.add(
            diagnostics,
            opportunities,
            ctx.fielders,
            runner_first,
        );
        if (45.0..50.0).contains(&ball.launch_speed) && (0.0..10.0).contains(&ball.launch_angle) {
            audit.hard_low_grounder_subset.add(
                diagnostics,
                opportunities,
                ctx.fielders,
                runner_first,
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::test_support::active_fielder;

    #[test]
    fn separates_candidate_filters_from_post_bounce_sampling() {
        let mut ball = BattedBall::default();
        ball.total_time = 1.0;
        ball.first_bounce_time = Some(0.0);
        ball.first_bounce_position = Some(PolarPosition::new(0.0, 0.0));
        ball.final_position = PolarPosition::new(100.0, 0.0);
        let mut fielder = active_fielder(Position::SS, 43.0, 0.0);
        fielder.info.running_speed = 2.0;
        fielder.info.prep_time = 0.4;
        let candidate = FielderCandidateDiagnostics {
            fielder_id: fielder.id,
            fielder_position: fielder.position,
            lane_eligible: false,
            distance_eligible: true,
            required_margin_sec: 0.0,
            search: None,
            air_probe: None,
            air_probe_coarse: None,
            interception_catch_type: None,
            interception_error_type: None,
            interception_time_sec: None,
            selected: false,
            selected_path: None,
        };
        let [live, relaxed, fine] = find_opportunities(&ball, &[(&fielder, &candidate)], 0.9);
        assert!(live.is_none() && relaxed.is_none());
        assert!((fine.unwrap().time_sec - 0.43).abs() < 1e-9);

        fielder.polar_position = PolarPosition::new(45.0, 0.0);
        let [live, relaxed, fine] = find_opportunities(&ball, &[(&fielder, &candidate)], 0.9);
        assert!(live.is_none());
        assert!((relaxed.unwrap().time_sec - 0.45).abs() < 1e-9);
        assert!((fine.unwrap().time_sec - 0.45).abs() < 1e-9);

        fielder.polar_position = PolarPosition::new(52.0, 0.0);
        assert!(
            find_opportunities(&ball, &[(&fielder, &candidate)], 0.9)
                .iter()
                .all(Option::is_none)
        );
    }
}
