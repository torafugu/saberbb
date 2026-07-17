use super::fielding_config::*;
use super::running_resolver::RunnersOnBase;
use super::throw_target_rules::*;
use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::ball::{BattedBall, FieldedBall, TrajectoryType};
use crate::domain::shared::game::BASE_DISTANCE;
use crate::domain::shared::game_state::{ActiveFielder, GameError};
use crate::domain::shared::player::{CatcherInfo, PitcherInfo, Position};
use crate::domain::shared::stadium::Base;
use crate::domain::util::{PolarPosition, calculate_polar_distance};
use crate::t;
use serde::{Deserialize, Serialize};
use std::f64::consts::SQRT_2;
use std::fmt::Debug;
use strum_macros::{AsRefStr, EnumString};

#[derive(Debug, Clone)]
pub struct DefenseTimeResult {
    pub defense_time: f64,
    pub final_fielder_position: Position,
}

// TODO: case of final fielder failed to catch
#[derive(Debug, Clone, Copy)]
pub struct DefenseTimeCalculator {
    ball_flight_speed_continue_distance: f64,
    cutoff_distance_coefficient: f64,
    touch_penalty_time: f64,
}

impl Default for DefenseTimeCalculator {
    fn default() -> Self {
        Self {
            ball_flight_speed_continue_distance: BALL_FLIGHT_SPEED_CONTINUE_DISTANCE,
            cutoff_distance_coefficient: CUTOFF_DISTANCE_COEFFICIENT,
            touch_penalty_time: TOUCH_PENALTY_TIME,
        }
    }
}

impl DefenseTimeCalculator {
    pub fn ball_flight_time(&self, distance: f64, initial_throw_speed: f64) -> f64 {
        if distance <= self.ball_flight_speed_continue_distance {
            return distance / initial_throw_speed;
        }

        // Beyond 30m, add a mild delay proportional to distance squared (air resistance penalty)
        // A 100m direct throw takes roughly 0.5–0.8s longer than the simple calculation
        let base_time = distance / initial_throw_speed;
        let penalty_factor =
            1.0 + (distance - self.ball_flight_speed_continue_distance).powi(2) * 0.0001;

        base_time * penalty_factor
    }

    pub fn direct_throw_time(
        &self,
        thrower: &ActiveFielder,
        from: &PolarPosition,
        to: &PolarPosition,
    ) -> f64 {
        let distance = calculate_polar_distance(from, to);
        thrower.info.prep_time + self.ball_flight_time(distance, thrower.info.throw_speed)
    }

    pub fn run_to_base_time(
        &self,
        fielder: &ActiveFielder,
        from: &PolarPosition,
        to: &PolarPosition,
    ) -> f64 {
        let distance = calculate_polar_distance(from, to);
        distance / fielder.info.running_speed
    }

    pub fn direct_play_time(
        &self,
        thrower: &ActiveFielder,
        from: &PolarPosition,
        target_base: Base,
        play_type: PlayType,
    ) -> f64 {
        let base_pos = target_base.polar_position();
        let throw_time = self.direct_throw_time(thrower, from, &base_pos);

        match play_type {
            PlayType::ForcePlay => throw_time,
            _ => throw_time + self.touch_penalty_time,
        }
    }

    pub fn infield_play_time(
        &self,
        thrower: &ActiveFielder,
        ball_pos: &PolarPosition,
        throw_target: &ThrowTarget,
    ) -> DefenseTimeResult {
        let base_pos = throw_target.base.polar_position();

        let throw_time =
            self.direct_play_time(thrower, ball_pos, throw_target.base, throw_target.play_type);

        if throw_target.play_type == PlayType::ForcePlay {
            let run_time = self.run_to_base_time(thrower, ball_pos, &base_pos);

            if run_time < throw_time {
                return DefenseTimeResult {
                    defense_time: run_time,
                    final_fielder_position: thrower.position,
                };
            }
        }

        DefenseTimeResult {
            defense_time: throw_time,
            final_fielder_position: throw_target.final_fielder_position,
        }
    }

    pub fn cutoff_position(&self, catch_pos: &PolarPosition, target_base: Base) -> PolarPosition {
        let base_pos = target_base.polar_position();
        let cutoff_distance = base_pos.distance
            + (catch_pos.distance - base_pos.distance) * self.cutoff_distance_coefficient;

        PolarPosition::new(cutoff_distance, catch_pos.angle)
    }

    pub fn relay_throw_time(
        &self,
        thrower: &ActiveFielder,
        catch_pos: &PolarPosition,
        target_base: Base,
        cutoff_fielder: &ActiveFielder,
    ) -> f64 {
        let base_pos = target_base.polar_position();
        let cutoff_pos = self.cutoff_position(catch_pos, target_base);

        let first_throw = self.direct_throw_time(thrower, catch_pos, &cutoff_pos);
        let second_flight_distance = calculate_polar_distance(&cutoff_pos, &base_pos);
        let second_throw = cutoff_fielder.info.prep_time
            + self.ball_flight_time(second_flight_distance, cutoff_fielder.info.throw_speed);

        first_throw + second_throw
    }

    pub fn best_outfield_throw_time(
        &self,
        thrower: &ActiveFielder,
        catch_pos: &PolarPosition,
        target_base: Base,
        play_type: PlayType,
        cutoff_fielder: Option<&ActiveFielder>,
    ) -> f64 {
        let direct_time = self.direct_play_time(thrower, catch_pos, target_base, play_type);

        if let Some(cutoff_fielder) = cutoff_fielder {
            let relay_time = self.relay_throw_time(thrower, catch_pos, target_base, cutoff_fielder);

            relay_time.min(direct_time)
        } else {
            direct_time
        }
    }

    pub fn multi_play_throw_time(
        &self,
        thrower: &ActiveFielder,
        from_base: Base,
        to_base: Base,
    ) -> Result<f64, GameError> {
        let distance = calculate_base_distance(from_base, to_base)?;

        Ok(thrower.info.prep_time + self.ball_flight_time(distance, thrower.info.throw_speed))
    }
}

fn find_fielder_by_position(
    fielders: &[ActiveFielder],
    position: Position,
) -> Result<&ActiveFielder, GameError> {
    let fielder = fielders
        .iter()
        .find(|i| i.is(position))
        .map(|i| i)
        .ok_or_else(|| GameError::NoPlayerFor(position.to_string()))?;

    Ok(fielder)
}

pub fn calculate_base_distance(from_base: Base, to_base: Base) -> Result<f64, GameError> {
    let distance = match from_base {
        Base::First => match to_base {
            Base::First => {
                return Err(GameError::SameTargetBase);
            }
            Base::Second => BASE_DISTANCE,
            Base::Third => BASE_DISTANCE * SQRT_2,
            Base::Home => BASE_DISTANCE,
        },
        Base::Second => match to_base {
            Base::First => BASE_DISTANCE,
            Base::Second => {
                return Err(GameError::SameTargetBase);
            }
            Base::Third => BASE_DISTANCE,
            Base::Home => BASE_DISTANCE * SQRT_2,
        },
        Base::Third => match to_base {
            Base::First => BASE_DISTANCE * SQRT_2,
            Base::Second => BASE_DISTANCE,
            Base::Third => {
                return Err(GameError::SameTargetBase);
            }
            Base::Home => BASE_DISTANCE,
        },
        Base::Home => match to_base {
            Base::First => BASE_DISTANCE,
            Base::Second => BASE_DISTANCE * SQRT_2,
            Base::Third => BASE_DISTANCE,
            Base::Home => {
                return Err(GameError::SameTargetBase);
            }
        },
    };
    Ok(distance)
}

// CONSTRAINT: Thrower must throw to another base in case double play
// CONSTRAINT: PlayType is always FourcePlay in case double play
fn double_play_defense_play(
    fielders: &[ActiveFielder],
    double_play_throw_target: &MultiPlayThrowTarget,
) -> Result<Option<DoublePlayDefensePlayResult>, GameError> {
    let thrower =
        find_fielder_by_position(fielders, double_play_throw_target.thrower_fielder_position)?;

    // TODO: case of final fielder failed to catch
    let final_fielder =
        find_fielder_by_position(fielders, double_play_throw_target.final_fielder_position)?;

    let calculator = DefenseTimeCalculator::default();

    let defense_time = calculator.multi_play_throw_time(
        thrower,
        double_play_throw_target.from_base,
        double_play_throw_target.to_base,
    )?;

    let double_play_defense_play_result = DoublePlayDefensePlayResult {
        throw_target_base: double_play_throw_target.to_base,
        thrower_fielder_id: thrower.id,
        thrower_fielder_position: double_play_throw_target.thrower_fielder_position,
        final_fielder_id: final_fielder.id,
        final_fielder_position: double_play_throw_target.final_fielder_position,
        defense_time: defense_time,
    };
    Ok(Some(double_play_defense_play_result))
}

// TODO: case of final fielder failed to catch
fn infield_grounder_defense_play(
    ctx: &PlayContext,
    throw_target: &ThrowTarget,
) -> Result<DefensePlayResult, GameError> {
    let calculator = DefenseTimeCalculator::default();

    let result = calculator.infield_play_time(
        ctx.try_catch_fielder,
        &ctx.fielded_ball.ball.polar_position,
        throw_target,
    );

    let final_player = find_fielder_by_position(ctx.fielders, result.final_fielder_position)?;
    let cuttoff_fielder_id =
        if let Some(cutoff_fielder_position) = throw_target.cutoff_fielder_position {
            Some(find_fielder_by_position(ctx.fielders, cutoff_fielder_position)?.id)
        } else {
            None
        };

    let defense_play_result = DefensePlayResult {
        time_to_field: ctx.fielded_ball.time_to_field,
        throw_target_base: throw_target.base,
        play_type: throw_target.play_type,
        final_fielder_id: final_player.id,
        final_fielder_position: result.final_fielder_position,
        cutoff_fielder_id: cuttoff_fielder_id,
        cutoff_fielder_position: throw_target.cutoff_fielder_position,
        defense_time: result.defense_time + ctx.fielded_ball.time_to_field,
    };
    Ok(defense_play_result)
}

// TODO: case of final fielder failed to catch
fn outfield_hit_tagup_defense_play(
    ctx: &PlayContext,
    throw_target: &ThrowTarget,
) -> Result<DefensePlayResult, GameError> {
    let calculator = DefenseTimeCalculator::default();

    let cutoff_fielder = throw_target
        .cutoff_fielder_position
        .map(|position| find_fielder_by_position(ctx.fielders, position))
        .transpose()?;

    // Catch + Throw time
    let defense_time = calculator.best_outfield_throw_time(
        ctx.try_catch_fielder,
        &ctx.fielded_ball.ball.polar_position,
        throw_target.base,
        throw_target.play_type,
        cutoff_fielder,
    ) + ctx.fielded_ball.time_to_field;

    let final_player = find_fielder_by_position(ctx.fielders, throw_target.final_fielder_position)?;
    let cutoff_fielder_id = if let Some(fielder) = cutoff_fielder {
        Some(fielder.id)
    } else {
        None
    };

    let defense_play_result = DefensePlayResult {
        time_to_field: ctx.fielded_ball.time_to_field,
        throw_target_base: throw_target.base,
        play_type: throw_target.play_type,
        final_fielder_id: final_player.id,
        final_fielder_position: throw_target.final_fielder_position,
        cutoff_fielder_id: cutoff_fielder_id,
        cutoff_fielder_position: throw_target.cutoff_fielder_position,
        defense_time: defense_time,
    };
    Ok(defense_play_result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, AsRefStr)]
pub enum PlayType {
    ForcePlay,
    TouchPlay,
    CatchPlay,
    ThrowPlay,
    CutOffPlay,
}
impl std::fmt::Display for PlayType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PlayType::ForcePlay => write!(f, "{}", t!("force_play")),
            PlayType::TouchPlay => write!(f, "{}", t!("touch_play")),
            PlayType::CatchPlay => write!(f, "{}", t!("catch_play")),
            PlayType::ThrowPlay => write!(f, "{}", t!("throw_play")),
            PlayType::CutOffPlay => write!(f, "{}", t!("cutoff_play")),
        }
    }
}

#[derive(Debug)]
pub struct StealDefensePlayResult {
    pub throw_target_base: Base,
    pub final_fielder_position: Position,
    pub defense_time: f64,
}

#[derive(Debug)]
pub struct PlayContext<'a> {
    pub runners: &'a RunnersOnBase,
    pub fielders: &'a [ActiveFielder],
    pub try_catch_fielder: &'a ActiveFielder,
    pub fielded_ball: &'a FieldedBall,
}

// CONSTRAINT: PlayType must be ForcePlay
#[derive(Debug)]
pub struct MultiPlayThrowTarget {
    pub from_base: Base,
    pub to_base: Base,
    pub thrower_fielder_position: Position,
    pub final_fielder_position: Position,
}

#[derive(Debug)]
pub struct MultiPlayThrowTargetPlan {
    pub from_base: Base,
    pub to_base: Base,
    pub thrower_fielder_position: CoverAssignment,
    pub final_fielder_position: CoverAssignment,
}

#[derive(Debug)]
pub struct ThrowTarget {
    pub base: Base,
    pub play_type: PlayType,
    pub final_fielder_position: Position,
    pub cutoff_fielder_position: Option<Position>,
}

#[derive(Debug)]
pub struct ThrowTargetPlan {
    pub base: Base,
    pub play_type: PlayType,
    pub final_fielder_position: CoverAssignment,
    pub cutoff_fielder_position: Option<CoverAssignment>,
}

#[derive(Debug)]
struct CoverDecision {
    middle_infield_ss_weight: f64,
    rng: Box<dyn RandomProvider>,
}
impl CoverDecision {
    fn resolve(
        &mut self,
        assignment: CoverAssignment,
        final_fielder_position: Position,
    ) -> Position {
        match assignment {
            CoverAssignment::Fixed(position) => position,

            CoverAssignment::MiddleInfieldRandom => {
                if self.rng.random() < self.middle_infield_ss_weight {
                    Position::SS
                } else {
                    Position::SB
                }
            }

            CoverAssignment::OppositeMiddleInfielder => match final_fielder_position {
                Position::SS => Position::SB,
                Position::SB => Position::SS,
                _ => {
                    if self.rng.random() < self.middle_infield_ss_weight {
                        Position::SS
                    } else {
                        Position::SB
                    }
                }
            },

            CoverAssignment::OppositeFirstInfielder => match final_fielder_position {
                Position::FB => Position::P,
                _ => Position::FB,
            },

            CoverAssignment::CutoffByOutfieldSide => match final_fielder_position {
                Position::RF => Position::SB,
                _ => Position::SS,
            },

            CoverAssignment::FinalFielderByOutfieldSide => match final_fielder_position {
                Position::RF => Position::SS,
                _ => Position::SB,
            },
        }
    }
}

fn resolve_multiplay_throw_target_plan(
    final_fielder_position: Position,
    plan: MultiPlayThrowTargetPlan,
    decision: &mut CoverDecision,
) -> MultiPlayThrowTarget {
    MultiPlayThrowTarget {
        from_base: plan.from_base,
        to_base: plan.to_base,
        thrower_fielder_position: decision
            .resolve(plan.thrower_fielder_position, final_fielder_position),
        final_fielder_position: decision
            .resolve(plan.final_fielder_position, final_fielder_position),
    }
}

fn resolve_throw_target_plan(
    final_fielder_position: Position,
    plan: ThrowTargetPlan,
    decision: &mut CoverDecision,
) -> ThrowTarget {
    ThrowTarget {
        base: plan.base,
        play_type: plan.play_type,
        final_fielder_position: decision
            .resolve(plan.final_fielder_position, final_fielder_position),
        cutoff_fielder_position: plan
            .cutoff_fielder_position
            .map(|assignment| decision.resolve(assignment, final_fielder_position)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverAssignment {
    Fixed(Position),

    // NOTE: Choose SS/SB by configured probability.
    MiddleInfieldRandom,

    // NOTE: If SS fields the ball, SB covers. If SB fields the ball, SS covers.
    OppositeMiddleInfielder,

    // NOTE: If FB fields the ball, P covers. If P fields the ball, FB covers.
    OppositeFirstInfielder,

    // NOTE: For throws from outfield: RF uses SB as cutoff, LF/CF uses SS.
    CutoffByOutfieldSide,

    // NOTE: For throws from outfield: RF uses SB as cutoff then SS, LF/CF uses SS then SB.
    FinalFielderByOutfieldSide,
}

fn select_multiplay_throw_target(
    ctx: &DefensePlayResult,
    rules: &[MultiplayThrowRule],
) -> Option<MultiPlayThrowTargetPlan> {
    rules
        .iter()
        .find(|rule| (rule.applies)(ctx))
        .map(|rule| (rule.target)(ctx))
        .expect("throw target rules must include a default rule")
}

fn select_throw_target(ctx: &PlayContext, rules: &[ThrowRule]) -> ThrowTargetPlan {
    rules
        .iter()
        .find(|rule| (rule.applies)(ctx))
        .map(|rule| (rule.target)(ctx))
        .expect("throw target rules must include a default rule")
}

fn judge_tagup_throw_target(ctx: &PlayContext, rng: Box<dyn RandomProvider>) -> ThrowTarget {
    let plan = select_throw_target(ctx, TAGUP_RULES);

    resolve_throw_target_plan(
        ctx.try_catch_fielder.position,
        plan,
        &mut CoverDecision {
            middle_infield_ss_weight: WEIGHT_SS_BASE_COVER,
            rng: rng,
        },
    )
}

fn judge_double_play_throw_target(
    ctx: &DefensePlayResult,
    rng: Box<dyn RandomProvider>,
) -> Option<MultiPlayThrowTarget> {
    if let Some(plan) = select_multiplay_throw_target(ctx, INFIELD_GROUNDER_DOUBLE_PLAY_RULES) {
        Some(resolve_multiplay_throw_target_plan(
            ctx.final_fielder_position,
            plan,
            &mut CoverDecision {
                middle_infield_ss_weight: WEIGHT_SS_BASE_COVER,
                rng: rng,
            },
        ))
    } else {
        None
    }
}

fn judge_infield_grounder_throw_target(
    ctx: &PlayContext,
    rng: Box<dyn RandomProvider>,
) -> ThrowTarget {
    let plan = select_throw_target(ctx, INFIELD_GROUNDER_RULES);

    resolve_throw_target_plan(
        ctx.try_catch_fielder.position,
        plan,
        &mut CoverDecision {
            middle_infield_ss_weight: WEIGHT_SS_BASE_COVER,
            rng: rng,
        },
    )
}

fn judge_outfield_hit_throw_target(ctx: &PlayContext, rng: Box<dyn RandomProvider>) -> ThrowTarget {
    let plan = select_throw_target(ctx, OUTFIELD_HIT_RULES);

    resolve_throw_target_plan(
        ctx.try_catch_fielder.position,
        plan,
        &mut CoverDecision {
            middle_infield_ss_weight: WEIGHT_SS_BASE_COVER,
            rng: rng,
        },
    )
}

#[derive(Debug)]
pub struct DoublePlayDefensePlayResult {
    pub throw_target_base: Base,
    pub thrower_fielder_id: i64,
    pub thrower_fielder_position: Position,
    pub final_fielder_id: i64,
    pub final_fielder_position: Position,
    pub defense_time: f64,
}

#[derive(Debug)]
pub struct DefensePlayResult {
    pub time_to_field: f64,
    pub throw_target_base: Base,
    pub play_type: PlayType,
    pub final_fielder_id: i64,
    pub final_fielder_position: Position,
    pub cutoff_fielder_id: Option<i64>,
    pub cutoff_fielder_position: Option<Position>,
    pub defense_time: f64,
}

pub fn evaluate_base_stealing(
    target_base: Base,     // Second (steal 2nd) or Third (steal 3rd)
    pitcher: &PitcherInfo, // Quick motion speed, pitch velocity
    catcher: &CatcherInfo, // Arm strength (pop time), control
    mut rng: Box<dyn RandomProvider>,
) -> StealDefensePlayResult {
    // 1. Defense side: total time from pitch to throw completion to 2nd (or 3rd)
    // Pitcher's motion time (1.0s for quick motion, ~1.3s for normal)
    let pitch_delivery_time = pitcher.delivery_motion_time;

    // Catcher's pop time (pro average is about 1.9–2.0s)
    // Varies with throw distance to the target base (2nd: ~38m, 3rd: ~27m)
    let home_pos = Base::Home.polar_position();
    let target_pos = target_base.polar_position();
    let throw_distance = calculate_polar_distance(&home_pos, &target_pos);

    let catcher_pop_time =
        catcher.fielder_info.prep_time + (throw_distance / catcher.fielder_info.throw_speed);

    let final_fielder_position = if rng.random() < WEIGHT_SS_BASE_COVER {
        Position::SS
    } else {
        Position::SB
    };

    // Total defense time = pitcher motion + catcher pop time + tag play (0.3s)
    let total_defense_time = pitch_delivery_time + catcher_pop_time + TOUCH_PENALTY_TIME;

    StealDefensePlayResult {
        throw_target_base: target_base,
        final_fielder_position: final_fielder_position,
        defense_time: total_defense_time,
    }
}

pub fn evaluate_double_play(
    ctx: &PlayContext,
    defense_play_result: &DefensePlayResult,
    rng: Box<dyn RandomProvider>,
) -> Result<Option<DoublePlayDefensePlayResult>, GameError> {
    let double_play_throw_target = judge_double_play_throw_target(defense_play_result, rng);
    if double_play_throw_target.is_none() {
        Ok(None)
    } else {
        if let Some(throw_target) = double_play_throw_target {
            let double_play_defense_play_result =
                double_play_defense_play(ctx.fielders, &throw_target)?;
            Ok(double_play_defense_play_result)
        } else {
            Ok(None)
        }
    }
}

// TODO: Consider double play by picking off
pub fn evaluate_defense_play(
    ctx: &PlayContext,
    rng: Box<dyn RandomProvider>,
) -> Result<DefensePlayResult, GameError> {
    if ctx.fielded_ball.ball.is_infield() {
        let throw_target = judge_infield_grounder_throw_target(ctx, rng);
        let defense_play_result = infield_grounder_defense_play(ctx, &throw_target)?;
        Ok(defense_play_result)
    } else {
        let throw_target = if ctx.fielded_ball.is_fly_catch {
            judge_tagup_throw_target(ctx, rng)
        } else {
            judge_outfield_hit_throw_target(ctx, rng)
        };

        let defense_play_result = outfield_hit_tagup_defense_play(ctx, &throw_target)?;
        Ok(defense_play_result)
    }
}

#[derive(Debug)]
pub struct BoundedBallResult {
    pub time_to_fumble: f64, // Total time until the fielder picks up the ball
    pub final_distance: f64, // Final distance where the ball stopped (or hit the fence)
}

pub fn find_closest_fielder<'f>(
    fielders: &'f [ActiveFielder],
    ball: &BattedBall,
) -> Result<&'f ActiveFielder, GameError> {
    // 1. Filter candidate fielders by whether the hit is infield or outfield
    let candidates: Vec<&ActiveFielder> = fielders
        .iter()
        .filter(|f| {
            match ball.trajectory {
                // For grounders, infielders chase until the ball rolls past the infield
                TrajectoryType::Grounder => {
                    if ball.is_infield() {
                        f.position.is_infielder()
                    } else {
                        f.position.is_outfielder()
                    }
                }
                // For fly balls and liners
                _ => {
                    if ball.is_shallow() {
                        // Shallow fly: both infielders and outfielders can chase
                        true
                    } else {
                        // Deep fly: only outfielders (LF, CF, RF) are candidates
                        f.position.is_outfielder()
                    }
                }
            }
        })
        .collect();

    // 2. Among the filtered candidates, select the closest fielder using law-of-cosines distance
    candidates
        .into_iter()
        .min_by(|a, b| {
            let dist_a = calculate_polar_distance(&a.polar_position, &ball.polar_position);
            let dist_b = calculate_polar_distance(&b.polar_position, &ball.polar_position);

            // Use partial_cmp safely since f64 is not a total order
            dist_a
                .partial_cmp(&dist_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .ok_or_else(|| GameError::NoPlayerFor("closest fielder".to_string()))
}

// TODO: The lane width should moved to fielder's ability.
// Determine whether a fielder is in the ball's trajectory lane (lateral coverage)
fn is_ball_in_fielder_lane(fielder: &ActiveFielder, ball_angle: f64) -> bool {
    // Set lateral coverage angle width by position
    let coverage_angle = match fielder.position {
        Position::P => 4.0, // Pitcher has narrow lateral range
        Position::FB | Position::TB => 6.0,
        Position::SB | Position::SS => 8.0, // Middle infield has wider range
        _ => 12.0,                          // Outfielders have widest range
    };

    // If the angle difference is within coverage range, the fielder is in the lane
    (fielder.angle() - ball_angle).abs() <= coverage_angle
}

#[derive(Debug, Clone)]
pub struct DefensiveChainResult<'a> {
    pub fielder: &'a ActiveFielder,
    pub ball: BattedBall,
}

// Evaluate fielders on the trajectory lane from front to back (revised over-the-head version)
pub fn process_defensive_chain<'a>(
    fielders: &'a [ActiveFielder],
    ball: &BattedBall,
) -> Result<DefensiveChainResult<'a>, GameError> {
    // 1. Sort fielders in the same lane by distance (closest first)
    let mut lane_fielders: Vec<&ActiveFielder> = fielders
        .iter()
        .filter(|f| is_ball_in_fielder_lane(f, ball.angle()))
        .filter(|f| f.distance() <= ball.distance())
        .collect();

    lane_fielders.sort_by(|a, b| {
        a.distance()
            .partial_cmp(&b.distance())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 2. Check fielders from front to back
    for fielder in lane_fielders {
        // Fielder's position (distance)
        let fielder_dist = fielder.distance();

        // Time when the ball passes that fielder's distance
        let ratio = fielder_dist / ball.distance();
        let ball_arrival_time = ball.hang_time * ratio;

        // Time for the fielder to move laterally to intercept the ball's line
        let dx = fielder_dist * (fielder.angle() - ball.angle()).to_radians().sin();
        let required_move_distance = dx.abs();
        let fielder_arrival_time =
            fielder.info.reaction + (required_move_distance / fielder.info.running_speed);

        // [Judgment A] Can the fielder reach the spot laterally before the ball passes through?
        if fielder_arrival_time <= ball_arrival_time {
            // ★ Core: calculate the ball's height as it passes over the fielder's head!
            // Calculate height at this fielder's distance
            let ball_height = ball.calculate_height_at_distance(fielder_dist);

            match ball.trajectory {
                // Case 1 & 2: Fly, pop-up, or high liner
                TrajectoryType::Fly | TrajectoryType::PopUp | TrajectoryType::Liner => {
                    if ball_height > MAX_REACH_HEIGHT {
                        // Angle and timing are right, but it's too high even for a jump catch!
                        continue;
                    }
                }
                // Grounders always have height 0 and never go over the head (proceed to catch)
                TrajectoryType::Grounder => {}
            }

            let mut ball_fielded_by_another = ball.clone();
            ball_fielded_by_another.hang_time = ball_arrival_time;

            return Ok(DefensiveChainResult {
                fielder: fielder,
                ball: ball_fielded_by_another,
            });
        }
    }

    // 3. Nobody touched it and it got through to the outfield (same as before: closest outfielder handles it)
    let final_closest = find_closest_fielder(fielders, ball)?;

    Ok(DefensiveChainResult {
        fielder: final_closest,
        ball: ball.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::random_provider::FixedRng;
    use crate::domain::shared::game_state::ActiveRunner;
    use crate::domain::shared::player::{
        FielderInfo, FielderType, PitcherStyle, RL, RunningSkills,
    };

    fn assert_near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {actual} to be near {expected}"
        );
    }

    fn fielder(position: Position, distance: f64, angle: f64) -> ActiveFielder {
        ActiveFielder {
            position,
            id: 0,
            info: FielderInfo {
                fielder_type: FielderType::Outfielder,
                throw_speed: 35.0,
                running_speed: 7.0,
                reaction: 0.4,
                prep_time: 0.6,
            },
            polar_position: PolarPosition::new(distance, angle),
        }
    }

    fn ball(
        trajectory: TrajectoryType,
        distance: f64,
        angle: f64,
        hang_time: f64,
        launch_speed_kmh: f64,
        launch_angle: f64,
    ) -> BattedBall {
        BattedBall::new(
            launch_speed_kmh,
            launch_angle,
            angle,
            distance,
            hang_time,
            trajectory,
        )
    }

    fn fielded_ball(
        ball: BattedBall,
        fielded_by: Position,
        time_to_field: f64,
        is_fly_catch: bool,
    ) -> FieldedBall {
        FieldedBall {
            ball,
            fielded_by,
            time_to_field,
            is_fly_catch,
        }
    }

    fn runners_on_base(
        runner_1st_speed: Option<f64>,
        runner_2nd_speed: Option<f64>,
        runner_3rd_speed: Option<f64>,
    ) -> RunnersOnBase {
        fn runner(speed: f64) -> ActiveRunner {
            ActiveRunner {
                id: 0,
                skills: RunningSkills {
                    speed: speed,
                    lead_distance: 0.0,
                    start_reaction: 0.1,
                },
            }
        }

        RunnersOnBase {
            batting_side: Some(RL::Left),
            batter_runner: Some(runner(7.0)),
            runner_1st: runner_1st_speed.map(runner),
            runner_2nd: runner_2nd_speed.map(runner),
            runner_3rd: runner_3rd_speed.map(runner),
        }
    }

    fn pitcher(delivery_motion_time: f64) -> PitcherInfo {
        PitcherInfo {
            pitcher_style: PitcherStyle::BalancedPitcher,
            velocity: 0.0,
            control: 0.0,
            stamina: 0.0,
            injury_proneness: 0.0,
            clutch: 0.0,
            hpp: 0.0,
            platoon_splitting: 0.0,
            delivery_motion_time,
            pitch_skills: vec![],
            fielder_info: FielderInfo {
                fielder_type: FielderType::Pitcher,
                throw_speed: 40.0,
                running_speed: 7.0,
                reaction: 0.5,
                prep_time: 0.65,
            },
        }
    }

    fn catcher(prep_time: f64, throw_speed: f64) -> CatcherInfo {
        CatcherInfo {
            fielder_info: FielderInfo {
                fielder_type: FielderType::Catcher,
                throw_speed,
                running_speed: 7.0,
                reaction: 0.5,
                prep_time,
            },
        }
    }

    fn fixed_rng() -> Box<dyn RandomProvider> {
        Box::new(FixedRng::new(0.1))
    }

    fn expected_steal_defense_time(
        target_base: Base,
        pitcher: &PitcherInfo,
        catcher: &CatcherInfo,
    ) -> f64 {
        let throw_distance =
            calculate_polar_distance(&Base::Home.polar_position(), &target_base.polar_position());

        pitcher.delivery_motion_time
            + catcher.fielder_info.prep_time
            + (throw_distance / catcher.fielder_info.throw_speed)
            + 0.3
    }

    fn default_fielders() -> [ActiveFielder; 9] {
        [
            fielder(Position::P, 18.44, 0.0),
            fielder(Position::C, 0.0, 0.0),
            fielder(Position::FB, 35.0, 33.0),
            fielder(Position::SB, 40.0, 18.0),
            fielder(Position::TB, 35.0, -33.0),
            fielder(Position::SS, 35.0, -33.0),
            fielder(Position::RF, 80.0, 26.0),
            fielder(Position::CF, 90.0, 0.0),
            fielder(Position::LF, 80.0, -26.0),
        ]
    }

    fn assert_target(target: ThrowTarget, expected_base: Base, expected_play_type: PlayType) {
        assert_eq!(target.base, expected_base);
        assert_eq!(target.play_type, expected_play_type);
    }

    #[test]
    fn fielder_new_sets_polar_position_and_skills() {
        let fielder = ActiveFielder {
            position: Position::CF,
            id: 0,
            info: FielderInfo {
                fielder_type: FielderType::Outfielder,
                throw_speed: 38.0,
                running_speed: 7.5,
                reaction: 0.3,
                prep_time: 0.5,
            },
            polar_position: PolarPosition::new(80.0, 0.0),
        };

        assert_eq!(fielder.position, Position::CF);
        assert_near(fielder.distance(), 80.0);
        assert_near(fielder.angle(), 0.0);
        assert_near(fielder.x(), 0.0);
        assert_near(fielder.y(), 80.0);
        assert_near(fielder.info.throw_speed, 38.0);
        assert_near(fielder.info.running_speed, 7.5);
        assert_near(fielder.info.reaction, 0.3);
        assert_near(fielder.info.prep_time, 0.5);
    }

    #[test]
    fn calculate_cutoff_position_keeps_throw_line_and_uses_midpoint_weight() {
        let catch_position = PolarPosition::new(90.0, -20.0);
        let calculator = DefenseTimeCalculator::default();

        let cutoff_position = calculator.cutoff_position(&catch_position, Base::Home);

        assert_near(cutoff_position.angle, catch_position.angle);
        assert_near(cutoff_position.distance, 90.0 * 0.45);
        assert_near(cutoff_position.x, 40.5 * (-20.0_f64).to_radians().sin());
        assert_near(cutoff_position.y, 40.5 * (-20.0_f64).to_radians().cos());
    }

    #[test]
    fn evaluate_base_stealing_to_second_returns_defense_time_and_covering_fielder() {
        let pitcher = pitcher(1.0);
        let catcher = catcher(0.4, 35.0);

        let result = evaluate_base_stealing(Base::Second, &pitcher, &catcher, fixed_rng());

        assert_eq!(result.throw_target_base, Base::Second);
        assert!(matches!(
            result.final_fielder_position,
            Position::SS | Position::SB
        ));
        assert_near(
            result.defense_time,
            expected_steal_defense_time(Base::Second, &pitcher, &catcher),
        );
    }

    #[test]
    fn evaluate_base_stealing_to_third_returns_shorter_throw_time() {
        let pitcher = pitcher(1.0);
        let catcher = catcher(0.4, 35.0);
        let second_result = evaluate_base_stealing(Base::Second, &pitcher, &catcher, fixed_rng());
        let third_result = evaluate_base_stealing(Base::Third, &pitcher, &catcher, fixed_rng());

        assert_eq!(third_result.throw_target_base, Base::Third);
        assert_near(
            third_result.defense_time,
            expected_steal_defense_time(Base::Third, &pitcher, &catcher),
        );
        assert!(third_result.defense_time < second_result.defense_time);
    }

    #[test]
    fn evaluate_double_play_returns_second_throw_to_first_after_force_at_second() {
        let fielders = default_fielders();
        let runners = runners_on_base(Some(6.5), None, Some(6.8));
        let ball = ball(TrajectoryType::Grounder, 35.0, -25.0, 1.0, 95.0, 4.0);
        let thrower = fielders
            .iter()
            .find(|fielder| fielder.is(Position::SS))
            .unwrap();
        let fielded_ball = fielded_ball(ball, thrower.position, 1.0, false);
        let throw_distance = calculate_polar_distance(
            &Base::Second.polar_position(),
            &Base::First.polar_position(),
        );
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: thrower,
            fielded_ball: &fielded_ball,
        };
        let first_play = DefensePlayResult {
            time_to_field: 1.0,
            throw_target_base: Base::Second,
            play_type: PlayType::ForcePlay,
            final_fielder_id: 0,
            final_fielder_position: Position::SS,
            cutoff_fielder_id: None,
            cutoff_fielder_position: None,
            defense_time: 1.2,
        };

        let result = evaluate_double_play(&ctx, &first_play, fixed_rng())
            .unwrap()
            .unwrap();
        let expected_defense_time = thrower.info.prep_time
            + DefenseTimeCalculator::default()
                .ball_flight_time(throw_distance, thrower.info.throw_speed);

        assert_eq!(result.throw_target_base, Base::First);
        assert_eq!(result.final_fielder_position, Position::FB);
        assert_near(result.defense_time, expected_defense_time);
    }

    #[test]
    fn evaluate_double_play_returns_error_when_thrower_is_missing() {
        let fielders = [fielder(Position::SS, 35.0, -33.0)];
        let runners = runners_on_base(Some(7.0), None, None);
        let ball = ball(TrajectoryType::Grounder, 35.0, -25.0, 1.0, 95.0, 4.0);
        let fielded_ball = fielded_ball(ball, fielders[0].position, 1.0, false);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &fielders[0],
            fielded_ball: &fielded_ball,
        };
        let first_play = DefensePlayResult {
            time_to_field: 1.0,
            throw_target_base: Base::Second,
            play_type: PlayType::ForcePlay,
            final_fielder_id: 0,
            final_fielder_position: Position::SB,
            cutoff_fielder_id: None,
            cutoff_fielder_position: None,
            defense_time: 1.2,
        };

        let result = evaluate_double_play(&ctx, &first_play, fixed_rng());

        assert!(
            matches!(result, Err(GameError::NoPlayerFor(position)) if position == Position::SB.to_string())
        );
    }

    #[test]
    fn try_catch_returns_out_when_fielder_arrives_before_airborne_ball_lands() {
        let center_fielder = fielder(Position::CF, 75.0, 0.0);
        let fly_ball = ball(TrajectoryType::Fly, 80.0, 0.0, 3.0, 120.0, 35.0);

        let result = center_fielder.try_catch(&fly_ball);

        assert!(result.is_fly_catch);
        assert_near(result.time_to_field, fly_ball.hang_time);
        assert_near(result.ball.distance(), 80.0);
        assert_near(result.ball.angle(), 0.0);
    }

    #[test]
    fn try_catch_returns_safe_and_bounded_distance_when_airborne_ball_falls_in() {
        let center_fielder = fielder(Position::CF, 60.0, 0.0);
        let mut liner = ball(TrajectoryType::Liner, 80.0, 0.0, 1.0, 100.0, 15.0);
        let original_distance = liner.distance();
        let original_hang_time = liner.hang_time;

        let result = center_fielder.try_catch(&mut liner);

        assert!(!result.is_fly_catch);
        assert!(result.time_to_field > original_hang_time);
        assert!(result.ball.distance() > original_distance);
        assert_near(result.ball.angle(), 0.0);
    }

    #[test]
    fn try_catch_treats_grounders_as_safe_for_later_base_race() {
        let shortstop = fielder(Position::SS, 30.0, -5.0);
        let grounder = ball(TrajectoryType::Grounder, 32.0, -5.0, 0.9, 95.0, 4.0);

        let result = shortstop.try_catch(&grounder);

        assert!(!result.is_fly_catch);
        assert_near(result.time_to_field, 0.4 + (2.0 / 7.0));
        assert_near(result.ball.distance(), 32.0);
        assert_near(result.ball.angle(), -5.0);
    }

    #[test]
    fn liner_beyond_fielder_adds_reaction_delay_and_falls_in() {
        let center_fielder = fielder(Position::CF, 80.0, 0.0);
        let mut liner_beyond_fielder = ball(TrajectoryType::Liner, 90.0, 0.0, 2.0, 110.0, 15.0);

        let result = center_fielder.try_catch(&mut liner_beyond_fielder);

        assert!(!result.is_fly_catch);
        assert!(result.time_to_field > liner_beyond_fielder.hang_time);
        assert!(result.ball.distance() > liner_beyond_fielder.distance());
    }

    #[test]
    fn find_closest_fielder_uses_infielders_for_short_grounders() {
        let fielders = [
            fielder(Position::SB, 35.0, 5.0),
            fielder(Position::LF, 42.0, 0.0),
            fielder(Position::CF, 70.0, 0.0),
        ];
        let grounder = ball(TrajectoryType::Grounder, 40.0, 0.0, 1.0, 90.0, 5.0);

        let closest = find_closest_fielder(&fielders, &grounder).unwrap();

        assert_eq!(closest.position, Position::SB);
    }

    #[test]
    fn find_closest_fielder_uses_outfielders_for_deep_airborne_balls() {
        let fielders = [
            fielder(Position::SB, 60.0, 0.0),
            fielder(Position::LF, 82.0, -10.0),
            fielder(Position::CF, 80.0, 0.0),
        ];
        let fly_ball = ball(TrajectoryType::Fly, 78.0, 0.0, 3.0, 120.0, 35.0);

        let closest = find_closest_fielder(&fielders, &fly_ball).unwrap();

        assert_eq!(closest.position, Position::CF);
    }

    #[test]
    fn is_ball_in_fielder_lane_uses_position_specific_coverage() {
        let pitcher = fielder(Position::P, 18.0, 0.0);
        let shortstop = fielder(Position::SS, 30.0, -7.5);
        let left_fielder = fielder(Position::LF, 80.0, -11.5);

        assert!(is_ball_in_fielder_lane(&pitcher, 4.0));
        assert!(!is_ball_in_fielder_lane(&pitcher, 4.1));
        assert!(is_ball_in_fielder_lane(&shortstop, 0.0));
        assert!(is_ball_in_fielder_lane(&left_fielder, 0.0));
        assert!(!is_ball_in_fielder_lane(&left_fielder, 12.1));
    }

    #[test]
    fn process_defensive_chain_returns_front_lane_fielder_for_reachable_grounder() {
        let fielders = [
            fielder(Position::SS, 28.0, 1.0),
            fielder(Position::CF, 80.0, 0.0),
        ];
        let grounder = ball(TrajectoryType::Grounder, 90.0, 0.0, 3.0, 95.0, 5.0);
        let grounder = grounder;
        let expected_arrival_time = grounder.hang_time * (28.0 / 90.0);

        let result = process_defensive_chain(&fielders, &grounder).unwrap();

        assert_eq!(result.fielder.position, Position::SS);
        assert_eq!(result.ball.trajectory, TrajectoryType::Grounder);
        assert_near(result.ball.hang_time, expected_arrival_time);
    }

    #[test]
    fn process_defensive_chain_skips_fielder_when_airborne_ball_is_over_reach() {
        let fielders = [
            fielder(Position::SS, 35.0, 0.0),
            fielder(Position::CF, 80.0, 0.0),
        ];
        let fly_ball = ball(TrajectoryType::Fly, 85.0, 0.0, 3.5, 130.0, 35.0);
        let expected_arrival_time = fly_ball.hang_time * (80.0 / 85.0);

        let result = process_defensive_chain(&fielders, &fly_ball).unwrap();

        assert_eq!(result.fielder.position, Position::CF);
        assert_eq!(result.ball.trajectory, TrajectoryType::Fly);
        assert_near(result.ball.hang_time, expected_arrival_time);
    }

    #[test]
    fn process_defensive_chain_falls_back_to_closest_fielder_when_lane_misses() {
        let fielders = [
            fielder(Position::SS, 30.0, -30.0),
            fielder(Position::CF, 78.0, 0.0),
        ];
        let grounder = ball(TrajectoryType::Grounder, 80.0, 25.0, 2.0, 90.0, 5.0);
        let original_hang_time = grounder.hang_time;

        let result = process_defensive_chain(&fielders, &grounder).unwrap();

        assert_eq!(result.fielder.position, Position::CF);
        assert_eq!(result.ball.trajectory, TrajectoryType::Grounder);
        assert_near(result.ball.hang_time, original_hang_time);
    }

    #[test]
    fn judge_optimal_target_general_chooses_home_force_with_bases_loaded_grounder() {
        let pitcher = fielder(Position::P, 18.0, 0.0);
        let grounder = ball(TrajectoryType::Grounder, 18.0, 0.0, 0.8, 95.0, 4.0);
        let fielders = default_fielders();
        let runners = runners_on_base(Some(7.0), Some(7.0), Some(7.0));
        let fielded_ball = fielded_ball(grounder, pitcher.position, 0.8, false);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &pitcher,
            fielded_ball: &fielded_ball,
        };

        let target = judge_infield_grounder_throw_target(&ctx, fixed_rng());

        assert_target(target, Base::Home, PlayType::ForcePlay);
    }

    #[test]
    fn judge_optimal_target_general_chooses_third_force_for_left_side_grounder() {
        let third_baseman = fielder(Position::TB, 32.0, -35.0);
        let grounder = ball(TrajectoryType::Grounder, 30.0, -35.0, 1.0, 95.0, 4.0);
        let fielders = default_fielders();
        let runners = runners_on_base(Some(7.0), Some(7.0), None);
        let fielded_ball = fielded_ball(grounder, third_baseman.position, 1.0, false);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &third_baseman,
            fielded_ball: &fielded_ball,
        };

        let target = judge_infield_grounder_throw_target(&ctx, fixed_rng());

        assert_target(target, Base::Third, PlayType::ForcePlay);
    }

    #[test]
    fn judge_optimal_target_general_throws_home_on_shallow_hit_with_lead_runner() {
        let center_fielder = fielder(Position::CF, 75.0, 0.0);
        let shallow_hit = ball(TrajectoryType::Liner, 70.0, 0.0, 3.0, 100.0, 15.0);
        let fielders = default_fielders();
        let runners = runners_on_base(None, None, Some(7.0));
        let fielded_ball = fielded_ball(shallow_hit, center_fielder.position, 3.0, false);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &center_fielder,
            fielded_ball: &fielded_ball,
        };

        let target = judge_outfield_hit_throw_target(&ctx, fixed_rng());

        assert_target(target, Base::Home, PlayType::TouchPlay);
    }

    #[test]
    fn judge_optimal_target_general_chooses_third_on_shallow_center_hit_with_runner_on_first() {
        let center_fielder = fielder(Position::CF, 75.0, 0.0);
        let shallow_hit = ball(TrajectoryType::Liner, 72.0, 5.0, 3.8, 100.0, 15.0);
        let fielders = default_fielders();
        let runners = runners_on_base(Some(7.0), None, None);
        let fielded_ball = fielded_ball(shallow_hit, center_fielder.position, 3.8, false);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &center_fielder,
            fielded_ball: &fielded_ball,
        };

        let target = judge_outfield_hit_throw_target(&ctx, fixed_rng());

        assert_target(target, Base::Third, PlayType::TouchPlay);
    }

    #[test]
    fn judge_optimal_target_general_chooses_second_for_deep_extra_base_hit() {
        let left_fielder = fielder(Position::LF, 90.0, -25.0);
        let deep_hit = ball(TrajectoryType::Fly, 95.0, -20.0, 5.0, 120.0, 35.0);
        let fielders = default_fielders();
        let runners = runners_on_base(None, None, None);
        let fielded_ball = fielded_ball(deep_hit, left_fielder.position, 5.0, false);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &left_fielder,
            fielded_ball: &fielded_ball,
        };

        let target = judge_outfield_hit_throw_target(&ctx, fixed_rng());

        assert_target(target, Base::Second, PlayType::TouchPlay);
    }

    #[test]
    fn evaluate_defense_play_handles_infield_grounder_to_first() {
        let first_baseman = fielder(Position::FB, 28.0, 40.0);
        let catch_position = PolarPosition::new(20.0, 35.0);
        let grounder = ball(
            TrajectoryType::Grounder,
            catch_position.distance,
            catch_position.angle,
            0.8,
            95.0,
            4.0,
        );
        let runners = runners_on_base(None, None, None);
        let fielders = default_fielders();
        let fielded_ball = fielded_ball(grounder, first_baseman.position, 0.8, false);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &first_baseman,
            fielded_ball: &fielded_ball,
        };

        let result = evaluate_defense_play(&ctx, fixed_rng()).unwrap();
        let throw_distance =
            calculate_polar_distance(&catch_position, &Base::First.polar_position());
        let expected_defense_time = fielded_ball.time_to_field
            + (first_baseman.info.prep_time + (throw_distance / first_baseman.info.throw_speed))
                .min(throw_distance / first_baseman.info.running_speed);

        assert_eq!(result.time_to_field, 0.8);
        assert_eq!(result.throw_target_base, Base::First);
        assert_eq!(result.play_type, PlayType::ForcePlay);
        assert_eq!(result.final_fielder_position, Position::FB);
        assert_eq!(result.cutoff_fielder_position, None);
        assert_near(result.defense_time, expected_defense_time);
    }

    #[test]
    fn evaluate_defense_play_handles_tagup_throw_home_with_cutoff() {
        let left_fielder = fielder(Position::LF, 78.0, -20.0);
        let catch_position = PolarPosition::new(78.0, -20.0);
        let shallow_hit = ball(
            TrajectoryType::Liner,
            catch_position.distance,
            catch_position.angle,
            3.2,
            100.0,
            15.0,
        );
        let runners = runners_on_base(None, None, Some(10.0));
        let fielders = default_fielders();
        let fielded_ball = fielded_ball(shallow_hit, left_fielder.position, 3.2, true);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &left_fielder,
            fielded_ball: &fielded_ball,
        };

        let result = evaluate_defense_play(&ctx, fixed_rng()).unwrap();
        let shortstop = fielders
            .iter()
            .find(|fielder| fielder.is(Position::SS))
            .unwrap();
        let expected_defense_time = fielded_ball.time_to_field
            + DefenseTimeCalculator::default().best_outfield_throw_time(
                &left_fielder,
                &catch_position,
                Base::Home,
                PlayType::TouchPlay,
                Some(shortstop),
            );

        assert_eq!(result.time_to_field, 3.2);
        assert_eq!(result.throw_target_base, Base::Home);
        assert_eq!(result.play_type, PlayType::TouchPlay);
        assert_eq!(result.final_fielder_position, Position::C);
        assert_eq!(result.cutoff_fielder_position, Some(Position::SS));
        assert_near(result.defense_time, expected_defense_time);
    }

    #[test]
    fn evaluate_defense_play_handles_deep_outfield_hit_to_second() {
        let center_fielder = fielder(Position::CF, 90.0, 0.0);
        let deep_hit = ball(TrajectoryType::Fly, 95.0, 0.0, 5.0, 120.0, 35.0);
        let runners = runners_on_base(None, None, None);
        let fielders = default_fielders();
        let fielded_ball = fielded_ball(deep_hit.clone(), center_fielder.position, 5.0, false);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &center_fielder,
            fielded_ball: &fielded_ball,
        };

        let result = evaluate_defense_play(&ctx, fixed_rng()).unwrap();
        let shortstop = fielders
            .iter()
            .find(|fielder| fielder.is(Position::SS))
            .unwrap();
        let expected_defense_time = fielded_ball.time_to_field
            + DefenseTimeCalculator::default().best_outfield_throw_time(
                &center_fielder,
                &deep_hit.polar_position,
                Base::Second,
                PlayType::TouchPlay,
                Some(shortstop),
            );

        assert_eq!(result.time_to_field, 5.0);
        assert_eq!(result.throw_target_base, Base::Second);
        assert_eq!(result.play_type, PlayType::TouchPlay);
        assert_eq!(result.final_fielder_position, Position::SB);
        assert_eq!(result.cutoff_fielder_position, Some(Position::SS));
        assert_near(result.defense_time, expected_defense_time);
    }
}
