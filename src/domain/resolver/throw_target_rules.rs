use super::fielding_config::{
    CUTOFF_NEEDED_DISTANCE_FOR_RUNNER_ON_FIRST, CUTOFF_NEEDED_DISTANCE_FOR_RUNNER_ON_THIRD,
    CUTOFF_NEEDED_TIME_TO_CATCH, DEEP_OUTFIELD_DISTANCE, SHALLOW_INFIELD_DISTANCE,
};
use super::fielding_resolver::{
    CoverAssignment, DefensePlayResult, MultiPlayThrowTargetPlan, PlayContext, PlayType,
    ThrowTargetPlan,
};
use crate::domain::shared::player::Position;
use crate::domain::shared::stadium::Base;

#[derive(Debug)]
pub struct ThrowRule {
    pub name: &'static str,
    pub applies: fn(&PlayContext) -> bool,
    pub target: fn(&PlayContext) -> ThrowTargetPlan,
}

pub const INFIELD_GROUNDER_RULES: &[ThrowRule] = &[
    ThrowRule {
        name: "bases_loaded_throw_home_force",
        applies: |ctx| ctx.runners.is_loaded(),
        target: |_| ThrowTargetPlan {
            base: Base::Home,
            play_type: PlayType::ForcePlay,
            final_fielder_position: CoverAssignment::Fixed(Position::C),
            cutoff_fielder_position: None,
        },
    },
    // TODO: Consider the case of protecting the 1-point lead
    ThrowRule {
        name: "shallow_grounder_throw_home_touch",
        applies: |ctx| {
            ctx.runners.has_runner_on(Base::Third)
                && ctx.fielded_ball.ball.distance() <= SHALLOW_INFIELD_DISTANCE
        },
        target: |_| ThrowTargetPlan {
            base: Base::Home,
            play_type: PlayType::TouchPlay,
            final_fielder_position: CoverAssignment::Fixed(Position::C),
            cutoff_fielder_position: None,
        },
    },
    ThrowRule {
        name: "first_and_second_left_side_force_third",
        applies: |ctx| {
            ctx.runners.has_first_and_second()
                && matches!(ctx.try_catch_fielder.position, Position::TB | Position::SS)
        },
        target: |_| ThrowTargetPlan {
            base: Base::Third,
            play_type: PlayType::ForcePlay,
            final_fielder_position: CoverAssignment::Fixed(Position::TB),
            cutoff_fielder_position: None,
        },
    },
    ThrowRule {
        name: "infield_in_force_first",
        applies: |ctx| {
            ctx.runners.has_runner_on(Base::First)
                && ctx.fielded_ball.ball.distance() <= SHALLOW_INFIELD_DISTANCE
                && matches!(
                    ctx.try_catch_fielder.position,
                    Position::P | Position::C | Position::FB | Position::TB
                )
        },
        target: |_| ThrowTargetPlan {
            base: Base::First,
            play_type: PlayType::ForcePlay,
            final_fielder_position: CoverAssignment::OppositeFirstInfielder,
            cutoff_fielder_position: None,
        },
    },
    ThrowRule {
        name: "first_and_up_the_middle_force_second",
        applies: |ctx| {
            ctx.runners.has_runner_on(Base::First)
                && matches!(
                    ctx.try_catch_fielder.position,
                    Position::SB | Position::SS | Position::P
                )
        },
        target: |_| ThrowTargetPlan {
            base: Base::Second,
            play_type: PlayType::ForcePlay,
            final_fielder_position: CoverAssignment::OppositeMiddleInfielder,
            cutoff_fielder_position: None,
        },
    },
    ThrowRule {
        name: "default_throw_first",
        applies: |_| true,
        target: |_| ThrowTargetPlan {
            base: Base::First,
            play_type: PlayType::ForcePlay,
            final_fielder_position: CoverAssignment::Fixed(Position::FB),
            cutoff_fielder_position: None,
        },
    },
];

pub const OUTFIELD_HIT_RULES: &[ThrowRule] = &[
    ThrowRule {
        // NOTE: if time_to_field is short and the outfielder is relatively shallow (within 80m), go for home
        name: "cutoff_throw_home_touch",
        applies: |ctx| {
            ctx.runners.has_runner_on(Base::Third)
                && ctx.fielded_ball.ball.distance() <= CUTOFF_NEEDED_DISTANCE_FOR_RUNNER_ON_THIRD
                && ctx.fielded_ball.time_to_field <= CUTOFF_NEEDED_TIME_TO_CATCH
        },
        target: |_| ThrowTargetPlan {
            base: Base::Home,
            play_type: PlayType::TouchPlay,
            final_fielder_position: CoverAssignment::Fixed(Position::C),
            cutoff_fielder_position: Some(CoverAssignment::CutoffByOutfieldSide),
        },
    },
    ThrowRule {
        // NOTE: Left-field hits often concede third, but right/center-field hits have a chance to nail them at third
        name: "cutoff_throw_third_touch",
        applies: |ctx| {
            ctx.runners.has_runner_on(Base::First)
                && ctx.fielded_ball.ball.distance() <= CUTOFF_NEEDED_DISTANCE_FOR_RUNNER_ON_FIRST
        },
        target: |_| ThrowTargetPlan {
            base: Base::Third,
            play_type: PlayType::TouchPlay,
            final_fielder_position: CoverAssignment::Fixed(Position::TB),
            cutoff_fielder_position: Some(CoverAssignment::MiddleInfieldRandom),
        },
    },
    ThrowRule {
        // NOTE: Prevent the batter from advancing to second
        name: "cutoff_throw_second_touch",
        applies: |ctx| ctx.fielded_ball.ball.distance() >= DEEP_OUTFIELD_DISTANCE,
        target: |_| ThrowTargetPlan {
            base: Base::Second,
            play_type: PlayType::TouchPlay,
            final_fielder_position: CoverAssignment::FinalFielderByOutfieldSide,
            cutoff_fielder_position: Some(CoverAssignment::CutoffByOutfieldSide),
        },
    },
    // NOTE: Throw back to the infield to settle the play (conveniently use the nearest infield base)
    // NOTE: For shallow hits, throw directly to second without a cutoff man
    ThrowRule {
        name: "default_throw_second",
        applies: |_| true,
        target: |_| ThrowTargetPlan {
            base: Base::Second,
            play_type: PlayType::TouchPlay,
            final_fielder_position: CoverAssignment::MiddleInfieldRandom,
            cutoff_fielder_position: None,
        },
    },
];

pub const TAGUP_RULES: &[ThrowRule] = &[
    ThrowRule {
        // NOTE: If the fly is too deep (e.g. 95m+), give up and throw to the infield (2nd base etc.)
        name: "cutoff_throw_home_touch",
        applies: |ctx| {
            ctx.runners.has_runner_on(Base::Third)
                && ctx.fielded_ball.ball.distance() <= DEEP_OUTFIELD_DISTANCE
        },
        target: |_| ThrowTargetPlan {
            base: Base::Home,
            play_type: PlayType::TouchPlay,
            final_fielder_position: CoverAssignment::Fixed(Position::C),
            cutoff_fielder_position: Some(CoverAssignment::CutoffByOutfieldSide),
        },
    },
    ThrowRule {
        // NOTE: Left-field fly: third baseman is either catching or off the base, so only throw to third on center/right-field flies
        name: "cutoff_throw_third_touch",
        applies: |ctx| {
            ctx.runners.has_runner_on(Base::Second)
                && matches!(ctx.try_catch_fielder.position, Position::CF | Position::RF)
        },
        target: |_| ThrowTargetPlan {
            base: Base::Third,
            play_type: PlayType::TouchPlay,
            final_fielder_position: CoverAssignment::Fixed(Position::TB),
            cutoff_fielder_position: Some(CoverAssignment::CutoffByOutfieldSide),
        },
    },
    // NOTE: Throw cleanly back to the infield (second) to end the play
    ThrowRule {
        name: "default_throw_second",
        applies: |_| true,
        target: |_| ThrowTargetPlan {
            base: Base::Second,
            play_type: PlayType::TouchPlay,
            final_fielder_position: CoverAssignment::MiddleInfieldRandom,
            cutoff_fielder_position: None,
        },
    },
];

#[derive(Debug)]
pub struct MultiplayThrowRule {
    pub name: &'static str,
    pub applies: fn(&DefensePlayResult) -> bool,
    pub target: fn(&DefensePlayResult) -> Option<MultiPlayThrowTargetPlan>,
}

pub const INFIELD_GROUNDER_DOUBLE_PLAY_RULES: &[MultiplayThrowRule] = &[
    MultiplayThrowRule {
        name: "first_throw_force_second",
        applies: |ctx| {
            ctx.throw_target_base == Base::First && ctx.final_fielder_position == Position::FB
        },
        target: |_| {
            Some(MultiPlayThrowTargetPlan {
                from_base: Base::First,
                to_base: Base::Second,
                thrower_fielder_position: CoverAssignment::Fixed(Position::FB),
                final_fielder_position: CoverAssignment::MiddleInfieldRandom,
            })
        },
    },
    MultiplayThrowRule {
        name: "second_throw_force_first",
        applies: |ctx| ctx.throw_target_base == Base::Second,
        target: |ctx| {
            Some(MultiPlayThrowTargetPlan {
                from_base: Base::Second,
                to_base: Base::First,
                thrower_fielder_position: CoverAssignment::Fixed(ctx.final_fielder_position),
                final_fielder_position: CoverAssignment::Fixed(Position::FB),
            })
        },
    },
    MultiplayThrowRule {
        name: "third_throw_force_first",
        applies: |ctx| ctx.throw_target_base == Base::Third,
        target: |ctx| {
            Some(MultiPlayThrowTargetPlan {
                from_base: Base::Third,
                to_base: Base::First,
                thrower_fielder_position: CoverAssignment::Fixed(ctx.final_fielder_position),
                final_fielder_position: CoverAssignment::Fixed(Position::FB),
            })
        },
    },
    // CONSTRAINT: Effect of draw-in infield is not cosidered. Assuming TB is enough close to third base.
    MultiplayThrowRule {
        name: "home_throw_force_third",
        applies: |ctx| {
            ctx.throw_target_base == Base::Home && ctx.final_fielder_position == Position::FB
        },
        target: |_| {
            Some(MultiPlayThrowTargetPlan {
                from_base: Base::Home,
                to_base: Base::Third,
                thrower_fielder_position: CoverAssignment::Fixed(Position::FB),
                final_fielder_position: CoverAssignment::Fixed(Position::TB),
            })
        },
    },
    // CONSTRAINT: Effect of draw-in infield is not cosidered. Assuming FB is enough close to third base.
    MultiplayThrowRule {
        name: "home_throw_force_first",
        applies: |ctx| {
            ctx.throw_target_base == Base::Home && ctx.final_fielder_position != Position::FB
        },
        target: |ctx| {
            Some(MultiPlayThrowTargetPlan {
                from_base: Base::Home,
                to_base: Base::First,
                thrower_fielder_position: CoverAssignment::Fixed(ctx.final_fielder_position),
                final_fielder_position: CoverAssignment::Fixed(Position::FB),
            })
        },
    },
    // NOTE: Throw cleanly back to the infield (second) to end the play
    MultiplayThrowRule {
        name: "throw_nothing",
        applies: |_| true,
        target: |_| None,
    },
];
