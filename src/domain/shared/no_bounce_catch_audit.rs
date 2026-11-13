//! Replay the first interception per eligible fielder on realized baseline singles.
use super::{
    ball::{BattedBall, TrajectoryType},
    game::BattingResult,
    game_state::ActiveFielder,
    player::Position,
};
use crate::domain::{
    random_provider::{FixedRng, RandomProvider, RealRng},
    resolver::fielding_physics::{
        CatchType, FieldPlayPlayResult, FielderRiskTolerance, evaluate_fielder_interception,
    },
};
use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Default, Serialize)]
struct Cohort {
    baseline_singles: u64,
    singles_with_risk_skipped_point: u64,
    replay_direct_out: u64,
    replay_without_direct_out: u64,
    direct_out_by_position: BTreeMap<String, u64>,
}

#[derive(Default, Serialize)]
pub struct NoBounceCatchAudit {
    all_singles: Cohort,
    by_trajectory: BTreeMap<String, Cohort>,
    outfield_pickup_grounder_singles: Cohort,
}

thread_local! {
    static ACTIVE: RefCell<Option<(NoBounceCatchAudit, RealRng)>> = const { RefCell::new(None) };
}

impl NoBounceCatchAudit {
    pub fn baseline_singles(&self) -> u64 {
        self.all_singles.baseline_singles
    }

    pub fn observe_game<T>(&mut self, game_seed: u64, run: impl FnOnce() -> T) -> T {
        struct Restore<'a> {
            previous: Option<(NoBounceCatchAudit, RealRng)>,
            target: &'a mut NoBounceCatchAudit,
        }
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                *self.target = ACTIVE.replace(self.previous.take()).unwrap().0;
            }
        }
        let rng = RealRng::from_seed(game_seed ^ 0xC7E0_B655_2B92_B44A);
        let previous = ACTIVE.replace(Some((std::mem::take(self), rng)));
        let _restore = Restore {
            previous,
            target: self,
        };
        run()
    }
}

enum Replay {
    NoRiskSkip,
    DirectOut(Position),
    NoDirectOut,
}

impl Cohort {
    fn add(&mut self, replay: &Replay) {
        self.baseline_singles += 1;
        match replay {
            Replay::NoRiskSkip => return,
            Replay::DirectOut(position) => {
                self.replay_direct_out += 1;
                *self
                    .direct_out_by_position
                    .entry(position.as_ref().into())
                    .or_default() += 1;
            }
            Replay::NoDirectOut => self.replay_without_direct_out += 1,
        }
        self.singles_with_risk_skipped_point += 1;
    }
}

fn replay(
    rng: &mut dyn RandomProvider,
    fielders: &[ActiveFielder],
    ball: &BattedBall,
    baseline: &FieldPlayPlayResult<'_>,
) -> Replay {
    let mut risk_skipped = false;
    let mut first: Option<(f64, bool, Position)> = None;
    // Candidate rows preserve original fielder order, including filtered fielders.
    for (fielder, candidate) in fielders.iter().zip(&baseline.candidate_diagnostics) {
        let Some(search) = candidate.search else {
            continue;
        };
        let interception = if search.reachable_samples > search.risk_ok_samples {
            risk_skipped = true;
            let mut fielder = fielder.clone();
            fielder.risk_tolerance = FielderRiskTolerance::Aggressive;
            // Reuse the original attempt's draw when available. New attempts
            // draw only from the independent diagnostic stream.
            let draw = search.fielding_error_draw.unwrap_or_else(|| rng.random());
            evaluate_fielder_interception(&mut FixedRng::new(draw), ball, &fielder).map(|catch| {
                (
                    catch.catch_time_sec,
                    catch.catch_type == CatchType::DirectFly && !catch.is_error(),
                )
            })
        } else {
            candidate.interception_time_sec.map(|time| {
                (
                    time,
                    candidate.interception_catch_type == Some(CatchType::DirectFly)
                        && candidate.interception_error_type.is_none(),
                )
            })
        };
        if let Some((time, direct)) = interception
            && first.is_none_or(|(best, _, _)| time.total_cmp(&best).is_lt())
        {
            first = Some((time, direct, fielder.position));
        }
    }
    match (risk_skipped, first) {
        (false, _) => Replay::NoRiskSkip,
        (true, Some((_, true, position))) => Replay::DirectOut(position),
        (true, _) => Replay::NoDirectOut,
    }
}

pub(crate) fn record(
    fielders: &[ActiveFielder],
    ball: &BattedBall,
    baseline: &FieldPlayPlayResult<'_>,
    batting_result: BattingResult,
) {
    if batting_result != BattingResult::Single {
        return;
    }
    ACTIVE.with_borrow_mut(|active| {
        let Some((audit, rng)) = active else { return };
        let replay = replay(rng, fielders, ball, baseline);
        let trajectory = ball.trajectory();
        audit.all_singles.add(&replay);
        audit
            .by_trajectory
            .entry(format!("{trajectory:?}"))
            .or_default()
            .add(&replay);
        let fielder = baseline.result();
        if trajectory == TrajectoryType::Grounder
            && fielder.fielder.position.is_outfielder()
            && fielder.catch_type == CatchType::FinalPickup
        {
            audit.outfield_pickup_grounder_singles.add(&replay);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        resolver::fielding_resolver::process_fielding, test_support::active_fielder,
        util::PolarPosition,
    };

    #[test]
    fn replay_pairs_the_fielder_error_draw_and_preserves_candidate_filters() {
        let ball = BattedBall {
            total_time: 0.5,
            first_bounce_time: Some(0.34),
            first_bounce_position: Some(PolarPosition::new(0.6, 0.0)),
            final_position: PolarPosition::new(1.0, 0.0),
            max_height: 1.0,
            ..BattedBall::default()
        };
        let mut fielder = active_fielder(Position::CF, 0.0, 0.0);
        fielder.info.running_speed = 1e9;
        fielder.info.prep_time = 0.14;
        fielder.info.reach_height = 3.0;
        fielder.info.catching = 0.8;
        fielder.risk_tolerance = FielderRiskTolerance::Conservative;
        let mut excluded = fielder.clone();
        excluded.polar_position = PolarPosition::new(2.0, 0.0);
        let fielders = [excluded, fielder];

        let clean = process_fielding(&mut FixedRng::new(0.5), &fielders, &ball).unwrap();
        assert_eq!(clean.result().catch_type, CatchType::BounceCatch);
        assert!(matches!(
            replay(&mut FixedRng::new(0.0), &fielders, &ball, &clean),
            Replay::DirectOut(Position::CF)
        ));

        // The bounce succeeds with 0.04, but the rushed direct attempt with the
        // same draw fails because the error probability is three times higher.
        let rushed = process_fielding(&mut FixedRng::new(0.04), &fielders, &ball).unwrap();
        assert_eq!(rushed.result().catch_type, CatchType::BounceCatch);
        assert!(matches!(
            replay(&mut FixedRng::new(0.99), &fielders, &ball, &rushed),
            Replay::NoDirectOut
        ));
    }
}
