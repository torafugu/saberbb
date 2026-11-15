//! Observe the live outfield play and compare throws to third without consuming RNG.
use super::{game::BattingResult, player::RL, stadium::Base};
use crate::domain::resolver::{
    fielding_resolver::{DefensePlayResult, PlayContext},
    fielding_timing::DefenseTimeCalculator,
};
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
    fastest_tested_route_to_third: Margin,
    third_opportunity_only_with_tested_routes: u64,
    third_opportunity_only_with_live_clock: u64,
}

#[derive(Default, Serialize)]
pub struct OutfieldAdvanceAudit {
    by_location_and_result: BTreeMap<&'static str, BTreeMap<String, BTreeMap<String, Routes>>>,
    unavailable_runner_times: u64,
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
    batting_side: RL,
    defense: &DefensePlayResult,
    result: BattingResult,
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
        let ball = &ctx.fielded_ball.ball;
        let field_time = defense.time_to_field;
        let location = if ball
            .fence_impact_time
            .is_some_and(|time| time <= field_time)
        {
            "wall_impact"
        } else if ball
            .first_bounce_time
            .is_some_and(|time| time <= field_time)
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
        let direct = defense.time_to_field
            + clock.direct_play_time(
                ctx.try_catch_fielder,
                &ctx.fielded_ball.catch_position,
                Base::Third,
                true,
            );
        routes.direct_throw_to_third.add(direct, third);
        let mut fastest = direct;
        if defense.throw_target_base == Base::Third {
            fastest = fastest.min(defense.defense_time);
        }
        if defense.throw_target_base == Base::Second {
            if let Some(receiver) = ctx
                .fielders
                .iter()
                .find(|f| f.id == defense.final_fielder_id)
            {
                let via_second = defense.defense_time
                    + clock.direct_play_time(
                        receiver,
                        &Base::Second.polar_position(),
                        Base::Third,
                        true,
                    );
                routes
                    .observed_return_to_second_then_throw_to_third
                    .add(via_second, third);
                fastest = fastest.min(via_second);
            }
        }
        routes.fastest_tested_route_to_third.add(fastest, third);
        let live_first = defense.defense_time > third;
        let tested_first = fastest > third;
        routes.third_opportunity_only_with_tested_routes += u64::from(tested_first && !live_first);
        routes.third_opportunity_only_with_live_clock += u64::from(live_first && !tested_first);
    });
}
