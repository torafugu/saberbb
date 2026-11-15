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
        CatchType, FieldPlayPlayResult, evaluate_fielder_interception_with_margin,
    },
};
use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Default, Serialize)]
struct MarginOutcome {
    singles_with_changed_attempt: u64,
    replay_direct_out: u64,
    replay_without_direct_out: u64,
    direct_out_by_position: BTreeMap<String, u64>,
}

#[derive(Default, Serialize)]
struct Cohort {
    baseline_singles: u64,
    by_margin_percent: BTreeMap<u8, MarginOutcome>,
}

const MARGIN_PERCENTS: [u8; 3] = [100, 50, 0];

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

#[derive(Clone, Copy, Default)]
struct Replay {
    changed_attempt: bool,
    direct_out: Option<Position>,
}

impl Cohort {
    fn add(&mut self, replays: &[Replay; 3]) {
        self.baseline_singles += 1;
        for (percent, replay) in MARGIN_PERCENTS.into_iter().zip(replays) {
            let outcome = self.by_margin_percent.entry(percent).or_default();
            if let Some(position) = replay.direct_out {
                outcome.replay_direct_out += 1;
                *outcome
                    .direct_out_by_position
                    .entry(position.as_ref().into())
                    .or_default() += 1;
            } else if replay.changed_attempt {
                outcome.replay_without_direct_out += 1;
            }
            outcome.singles_with_changed_attempt += u64::from(replay.changed_attempt);
        }
    }
}

fn replay(
    rng: &mut dyn RandomProvider,
    fielders: &[ActiveFielder],
    ball: &BattedBall,
    baseline: &FieldPlayPlayResult<'_>,
) -> [Replay; 3] {
    let mut replays = [Replay::default(); 3];
    let mut first: [Option<(f64, bool, Position)>; 3] = [None; 3];
    // Candidate rows preserve original fielder order, including filtered fielders.
    for (fielder, candidate) in fielders.iter().zip(&baseline.candidate_diagnostics) {
        let Some(search) = candidate.search else {
            continue;
        };
        let original = candidate.interception_time_sec.map(|time| {
            (
                time,
                candidate.interception_catch_type == Some(CatchType::DirectFly)
                    && candidate.interception_error_type.is_none(),
            )
        });
        let mut draw = search.fielding_error_draw;
        for (index, percent) in MARGIN_PERCENTS.into_iter().enumerate() {
            let margin = candidate.required_margin_sec * f64::from(percent) / 100.0;
            let interception = if percent < 100
                && search.reachable_samples > search.risk_ok_samples
                && search.max_air_margin_sec.is_some_and(|best| best >= margin)
            {
                // Share one draw per fielder across both replays. At zero margin
                // the fallback draws occur in the same order as in the original audit.
                let value = *draw.get_or_insert_with(|| rng.random());
                let (catch, diagnostics) = evaluate_fielder_interception_with_margin(
                    &mut FixedRng::new(value),
                    ball,
                    fielder,
                    margin,
                );
                replays[index].changed_attempt |=
                    diagnostics.last_evaluated_time_sec != search.last_evaluated_time_sec;
                catch.map(|catch| {
                    (
                        catch.catch_time_sec,
                        catch.catch_type == CatchType::DirectFly && !catch.is_error(),
                    )
                })
            } else {
                original
            };
            if let Some((time, direct)) = interception
                && first[index].is_none_or(|(best, _, _)| time.total_cmp(&best).is_lt())
            {
                first[index] = Some((time, direct, fielder.position));
            }
        }
    }
    for (result, winner) in replays.iter_mut().zip(first) {
        result.direct_out = winner.and_then(|(_, direct, position)| direct.then_some(position));
    }
    replays
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
        resolver::{
            fielding_physics::{FielderRiskTolerance, with_direct_catch_margin_scale},
            fielding_resolver::process_fielding,
        },
        test_support::active_fielder,
        util::PolarPosition,
    };

    #[test]
    fn replay_pairs_the_fielder_error_draw_and_preserves_candidate_filters() {
        with_direct_catch_margin_scale(1.0, check_replay);
    }

    fn check_replay() {
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
        let clean_replay = replay(&mut FixedRng::new(0.0), &fielders, &ball, &clean);
        assert_eq!(
            clean_replay.map(|r| r.direct_out),
            [None, Some(Position::CF), Some(Position::CF)]
        );

        let shorter_flight = BattedBall {
            first_bounce_time: Some(0.29),
            ..ball.clone()
        };
        let shorter_baseline =
            process_fielding(&mut FixedRng::new(0.5), &fielders, &shorter_flight).unwrap();
        assert_eq!(shorter_baseline.result().catch_type, CatchType::BounceCatch);
        let shorter_replay = replay(
            &mut FixedRng::new(0.0),
            &fielders,
            &shorter_flight,
            &shorter_baseline,
        );
        assert_eq!(
            shorter_replay.map(|r| r.direct_out),
            [None, None, Some(Position::CF)]
        );

        // The bounce succeeds with 0.04, but the rushed direct attempt with the
        // same draw fails because the error probability is three times higher.
        let rushed = process_fielding(&mut FixedRng::new(0.04), &fielders, &ball).unwrap();
        assert_eq!(rushed.result().catch_type, CatchType::BounceCatch);
        let rushed_replay = replay(&mut FixedRng::new(0.99), &fielders, &ball, &rushed);
        assert_eq!(rushed_replay.map(|r| r.direct_out), [None; 3]);
    }
}
