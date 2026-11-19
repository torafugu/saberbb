//! Diagnose catches and first-base timing on completed low line-drive singles.
use super::{
    ball::BattedBall,
    game::BattingResult,
    no_bounce_catch_audit,
    player::{Position, RL},
    stadium::Base,
};
use crate::domain::{
    random_provider::RealRng,
    resolver::{
        fielding_physics::{FieldPlayPlayResult, RightFieldFirstBaseSnapshot},
        fielding_resolver::PlayContext,
        fielding_timing::DefenseTimeCalculator,
        running_resolver::RunnerAdvanceResult,
    },
};
use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap};

const EXIT_SPEED_MS: std::ops::Range<f64> = 40.0..47.5;
const LAUNCH_ANGLE_DEG: std::ops::Range<f64> = 10.0..15.0;
type Counts = BTreeMap<&'static str, u64>;

#[derive(Default, Serialize)]
pub struct LowLineDriveFieldingAudit {
    total: Counts,
    by_exit_speed_ms: BTreeMap<&'static str, Counts>,
    by_fielding_position: BTreeMap<String, Counts>,
}

thread_local! {
    static ACTIVE: RefCell<Option<(LowLineDriveFieldingAudit, RealRng)>> = const { RefCell::new(None) };
}

impl LowLineDriveFieldingAudit {
    pub fn matches(ball: &BattedBall) -> bool {
        Self::matches_launch(ball.launch_speed, ball.launch_angle)
    }

    pub fn matches_launch(exit_speed_ms: f64, launch_angle_deg: f64) -> bool {
        EXIT_SPEED_MS.contains(&exit_speed_ms) && LAUNCH_ANGLE_DEG.contains(&launch_angle_deg)
    }

    pub fn singles(&self) -> u64 {
        self.total.get("singles").copied().unwrap_or(0)
    }

    pub fn observe_game<T>(&mut self, game_seed: u64, run: impl FnOnce() -> T) -> T {
        struct Restore<'a> {
            previous: Option<(LowLineDriveFieldingAudit, RealRng)>,
            target: &'a mut LowLineDriveFieldingAudit,
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

    pub fn merge(&mut self, other: Self) {
        fn add(target: &mut Counts, from: Counts) {
            for (key, value) in from {
                *target.entry(key).or_default() += value;
            }
        }
        add(&mut self.total, other.total);
        for (speed, counts) in other.by_exit_speed_ms {
            add(self.by_exit_speed_ms.entry(speed).or_default(), counts);
        }
        for (position, counts) in other.by_fielding_position {
            add(self.by_fielding_position.entry(position).or_default(), counts);
        }
    }
}

struct FirstBaseClocks {
    field_ready_sec: f64,
    batter_sec: f64,
    ideal_defense_sec: f64,
    late_cover_defense_sec: f64,
}

fn first_base_clocks(ctx: &PlayContext<'_>, batting_side: RL) -> Option<FirstBaseClocks> {
    let thrower = ctx.try_catch_fielder;
    let field_ready_sec = ctx.fielded_ball.time_to_field;
    let batter_sec = ctx
        .runners
        .batter_runner_time_to(Base::First, true, batting_side)
        .ok()?;
    if !batter_sec.is_finite() || batter_sec <= 0.0 {
        return None;
    }
    let cover_position = if thrower.position == Position::FB {
        Position::P
    } else {
        Position::FB
    };
    let cover = ctx.fielders.iter().find(|f| f.position == cover_position)?;
    let origin = ctx.fielded_ball.catch_position;
    let calculator = DefenseTimeCalculator::default();
    let (ball_arrival, cover_duration) = calculator.right_field_first_clocks(
        thrower,
        RightFieldFirstBaseSnapshot {
            catch_position: origin,
            cover_start: cover.polar_position,
            cover_prep_sec: cover.info.prep_time,
            cover_speed_ms: cover.info.running_speed,
        },
        field_ready_sec,
    )?;
    let mut ideal_defense_sec = ball_arrival;
    let mut late_cover_defense_sec = ball_arrival.max(field_ready_sec + cover_duration);
    if thrower.position.is_infielder()
        && thrower.info.running_speed.is_finite()
        && thrower.info.running_speed > 0.0
    {
        let carry = field_ready_sec
            + calculator.run_to_base_time(thrower, &origin, &Base::First.polar_position());
        ideal_defense_sec = ideal_defense_sec.min(carry);
        late_cover_defense_sec = late_cover_defense_sec.min(carry);
    }
    Some(FirstBaseClocks {
        field_ready_sec,
        batter_sec,
        ideal_defense_sec,
        late_cover_defense_sec,
    })
}

pub(crate) fn record(
    ctx: &PlayContext<'_>,
    baseline: &FieldPlayPlayResult<'_>,
    batting_side: RL,
    advance: &RunnerAdvanceResult,
) {
    let ball = &ctx.fielded_ball.ball;
    if advance.batting_result != BattingResult::Single || !LowLineDriveFieldingAudit::matches(ball) {
        return;
    }
    ACTIVE.with_borrow_mut(|active| {
        let Some((audit, rng)) = active else { return };
        let caught = no_bounce_catch_audit::replay(rng, ctx.fielders, ball, baseline)
            .map(|replay| replay.direct_out.is_some());
        let candidates = &baseline.candidate_diagnostics;
        let reachable = candidates.iter().any(|candidate| {
            candidate
                .search
                .is_some_and(|search| search.reachable_samples > 0)
        });
        let mut probe_reachable = candidates.iter().filter(|candidate| {
            candidate
                .air_probe
                .is_some_and(|probe| probe.reachable_samples > 0)
        });
        let any_probe_reachable = probe_reachable.clone().next().is_some();
        let only_filtered_probe_reachable = any_probe_reachable
            && !probe_reachable.any(|candidate| candidate.lane_eligible && candidate.distance_eligible);
        let batter_was_target = advance.throw_target_base == Base::First
            && advance
                .target_runner
                .zip(ctx.runners.batter_runner)
                .is_some_and(|(target, batter)| target.id == batter.id);
        let clocks = first_base_clocks(ctx, batting_side);
        let late_cover_out = clocks
            .as_ref()
            .is_some_and(|c| c.late_cover_defense_sec < c.batter_sec);
        let metrics = [
            ("singles", true),
            ("full_air_probe_reachable", any_probe_reachable),
            ("full_air_probe_only_filtered_reachable", only_filtered_probe_reachable),
            ("live_search_air_reachable", reachable),
            ("direct_out_at_current_margin", caught[0]),
            ("direct_out_at_half_margin", caught[1]),
            ("direct_out_at_zero_margin", caught[2]),
            ("recorded_batter_target_at_first", batter_was_target),
            ("first_base_clock_available", clocks.is_some()),
            ("field_ready_at_or_after_batter_first",
                clocks.as_ref().is_some_and(|c| c.field_ready_sec >= c.batter_sec)),
            ("first_base_out_receiver_ready",
                clocks.as_ref().is_some_and(|c| c.ideal_defense_sec < c.batter_sec)),
            ("first_base_out_cover_starts_at_fielding", late_cover_out),
            ("first_base_out_cover_starts_at_fielding_margin_ge_025_sec",
                clocks.as_ref().is_some_and(|c| c.batter_sec - c.late_cover_defense_sec >= 0.25)),
            ("either_zero_margin_direct_out_or_first_base_out_with_late_cover", caught[2] || late_cover_out),
        ];
        let speed = if ball.launch_speed < 45.0 {
            "[40,45)"
        } else {
            "[45,47.5)"
        };
        let position = ctx.try_catch_fielder.position.as_ref().to_owned();
        for group in [
            &mut audit.total,
            audit.by_exit_speed_ms.entry(speed).or_default(),
            audit.by_fielding_position.entry(position).or_default(),
        ] {
            for (key, flag) in metrics {
                *group.entry(key).or_default() += u64::from(flag);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        resolver::running_resolver::RunnersOnBase,
        shared::ball::FieldedBall,
        test_support::{active_fielder, active_runner},
        util::PolarPosition,
    };

    #[test]
    fn first_base_uses_the_fielding_point_and_accounts_for_cover_travel() {
        let runners = RunnersOnBase {
            batter_runner: Some(active_runner(1)),
            ..Default::default()
        };
        let fielders = [
            active_fielder(Position::RF, 65.0, 45.0),
            active_fielder(Position::FB, 55.0, 45.0),
        ];
        let fielded_ball = FieldedBall {
            ball: BattedBall {
                final_position: PolarPosition::new(90.0, 45.0),
                ..BattedBall::default()
            },
            fielded_by: Position::RF,
            catch_position: PolarPosition::new(31.0, 45.0),
            time_to_field: 1.0,
            is_fly_catch: false,
        };
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &fielders[0],
            fielded_ball: &fielded_ball,
        };
        let clocks = first_base_clocks(&ctx, RL::Right).unwrap();
        let calculator = DefenseTimeCalculator::default();
        let expected = clocks.field_ready_sec
            + calculator.direct_play_time(
                &fielders[0], &fielded_ball.catch_position, Base::First, false,
            );
        assert_eq!(clocks.ideal_defense_sec, expected);
        assert!(clocks.ideal_defense_sec < clocks.batter_sec);
        assert!(clocks.late_cover_defense_sec >= clocks.batter_sec);
    }

    #[test]
    fn launch_band_includes_only_its_lower_edges() {
        let ball = BattedBall {
            launch_speed: 40.0,
            launch_angle: 10.0,
            ..BattedBall::default()
        };
        assert!(LowLineDriveFieldingAudit::matches(&ball));
        for (speed, angle) in [(39.99, 10.0), (47.5, 10.0), (40.0, 9.99), (40.0, 15.0)] {
            assert!(!LowLineDriveFieldingAudit::matches(&BattedBall {
                launch_speed: speed,
                launch_angle: angle,
                ..ball
            }));
        }
    }
}
