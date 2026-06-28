use super::running_resolver::RunnersOnBase;
use crate::domain::shared::ball::{Ball, TrajectoryType};
use crate::domain::shared::game::BASE_DISTANCE;
use crate::domain::shared::game_state::GameError;
use crate::domain::shared::player::{Fielder, Position};
use crate::domain::shared::stadium::Base;
use crate::domain::util::{PolarPosition, calculate_polar_distance};
use std::f64::consts::SQRT_2;

// TODO: fence distance should be retrieved from the stadium
const FENCE_DISTANCE: f64 = 100.0; // Stadium fence distance (assumed 100m)
const FENCE_BOUNCE_COEFF: f64 = 0.25; // Fence bounce coefficient (grounder cushion is quite damped)
const DEEP_OUTFIELD_DISTANCE: f64 = 90.0;
const SHALLOW_INFIELD_DISTANCE: f64 = 25.0;
const CUTOFF_NEEDED_DISTACE_FOR_RUNNER_ON_THIRD: f64 = 80.0;
const CUTOFF_NEEDED_DISTACE_FOR_RUNNER_ON_FIRST: f64 = 75.0;
const CUTOFF_NEEDED_TIME_TO_CATCH: f64 = 3.5;
// Example: outfield at 90m, base at 0m (home) → place cutoff around 35–40m
const CUTOFF_DISTANCE_COEFFICIENT: f64 = 0.45;

// Base distance: assume top speed can be maintained up to 30m
const BALL_FLIGHT_SPEED_CONTINUE_DISTANCE: f64 = 30.0;

const LINER_REACTION_TIME: f64 = 0.15; // TODO: should be moved to Player ability
const TOUCH_PENALTY_TIME: f64 = 0.3;
const FIRST_BOUNCE_TIME: f64 = 0.5; // At least 0.5s after the first bounce

// Maximum jump catch height for a fielder (2.5m)
const MAX_REACH_HEIGHT: f64 = 2.5; // TODO: Should be changed to Player's ability

const WEIGHT_SS_BASE_COVER: f64 = 0.3;
const WEIGHT_IS_LOADED_TARGET_THIRD: f64 = 0.3;

fn find_fielder_by_position(
    fielders: &[Fielder],
    position: Position,
) -> Result<&Fielder, GameError> {
    let fielder = fielders
        .iter()
        .find(|i| i.is(position.clone()))
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

// Calculate actual ball flight time based on distance
fn calculate_ball_flight_time(distance: f64, initial_throw_speed: f64) -> f64 {
    if distance <= BALL_FLIGHT_SPEED_CONTINUE_DISTANCE {
        return distance / initial_throw_speed;
    }

    // Beyond 30m, add a mild delay proportional to distance squared (air resistance penalty)
    // A 100m direct throw takes roughly 0.5–0.8s longer than the simple calculation
    let base_time = distance / initial_throw_speed;
    let penalty_factor = 1.0 + (distance - BALL_FLIGHT_SPEED_CONTINUE_DISTANCE).powi(2) * 0.0001;

    base_time * penalty_factor
}

// Determine the optimal cutoff man position based on catch position and target base
fn calculate_cutoff_position(catch_pos: &PolarPosition, target_base: Base) -> PolarPosition {
    let base_pos = target_base.polar_position();

    // Keep the angle aligned with the outfielder's direction (to stay on the direct line)
    let cutoff_angle = catch_pos.angle;

    // Place the cutoff man at a good relay point between the target base and the outfield
    let target_r = base_pos.distance;
    let cutoff_distance = target_r + (catch_pos.distance - target_r) * CUTOFF_DISTANCE_COEFFICIENT;

    PolarPosition::new(cutoff_distance, cutoff_angle)
}

fn calculate_relay_play_time(
    throw_fielder: &Fielder,
    catch_pos: &PolarPosition,
    target_base: Base,
    cutoff_fielder: &Fielder, // Infielder data for the cutoff relay
) -> f64 {
    let base_pos = target_base.polar_position();

    // --- Pattern A: Direct throw ---
    let direct_dist = calculate_polar_distance(&catch_pos, &base_pos);
    let direct_flight_time = calculate_ball_flight_time(direct_dist, throw_fielder.throw_speed);
    let total_direct_time = throw_fielder.prep_time + direct_flight_time;

    // --- Pattern B: Cutoff relay play ---
    let cutoff_pos = calculate_cutoff_position(catch_pos, target_base);

    // Outfielder → cutoff man throw time
    let dist_1st = calculate_polar_distance(&catch_pos, &cutoff_pos);
    let flight_time_1st = calculate_ball_flight_time(dist_1st, throw_fielder.throw_speed);

    // Cutoff man → base throw time
    let dist_2nd = calculate_polar_distance(&cutoff_pos, &base_pos);
    let flight_time_2nd = calculate_ball_flight_time(dist_2nd, cutoff_fielder.throw_speed);

    // Total relay time = outfielder prep + 1st flight + cutoff prep + 2nd flight
    let total_relay_time =
        throw_fielder.prep_time + flight_time_1st + cutoff_fielder.prep_time + flight_time_2nd;

    // Pick the better option
    if total_relay_time < total_direct_time {
        total_relay_time // Cutoff route is faster (ball doesn't slow as much)
    } else {
        total_direct_time // Direct throw is faster (shallow fly, tag-up situations, etc.)
    }
}

fn double_play_defense_play(
    fielders: &[Fielder],
    double_play_throw_target: &DoublePlayThrowTarget,
) -> Result<Option<DoublePlayDefencePlayResult>, GameError> {
    let distance_to_base = calculate_base_distance(
        double_play_throw_target.from_base,
        double_play_throw_target.to_base,
    )?;

    let thorower_fielder = find_fielder_by_position(
        fielders,
        double_play_throw_target.thorower_fielder_position.clone(),
    )?;
    // TODO: case of final fielder failed to catch
    let _final_fielder = find_fielder_by_position(
        fielders,
        double_play_throw_target.final_fielder_position.clone(),
    )?;

    // CONSTRAINT: Thorower must throw to another base in case double play
    // CONSTRAINT: PlayType is always FourcePlay in case double play
    let defense_play_time = thorower_fielder.prep_time
        + calculate_ball_flight_time(distance_to_base, thorower_fielder.throw_speed);

    // let mut final_fielder_position = throw_target.final_fielder_position.clone();

    let double_play_defence_play_result = DoublePlayDefencePlayResult {
        throw_target_base: double_play_throw_target.to_base,
        final_fielder_position: double_play_throw_target.final_fielder_position.clone(),
        defense_time: defense_play_time,
    };
    Ok(Some(double_play_defence_play_result))
}

// TODO: case of final fielder failed to catch
fn infield_grounder_defense_play(
    ctx: &PlayContext,
    throw_target: &ThrowTarget,
) -> Result<DefencePlayResult, GameError> {
    let distance_to_base = calculate_polar_distance(
        &ctx.ball.polar_position,
        &throw_target.base.polar_position(),
    );

    let mut final_fielder_position = throw_target.final_fielder_position.clone();

    let time_to_final_fielder = ctx.try_catch_fielder.prep_time
        + calculate_ball_flight_time(distance_to_base, ctx.try_catch_fielder.throw_speed);
    let defense_play_time = if throw_target.play_type == PlayType::ForcePlay {
        let time_via_run = distance_to_base / ctx.try_catch_fielder.running_speed;

        if time_to_final_fielder > time_via_run {
            final_fielder_position = ctx.try_catch_fielder.position.clone();
            time_via_run
        } else {
            time_to_final_fielder
        }
    } else {
        time_to_final_fielder + TOUCH_PENALTY_TIME
    };

    let defence_play_result = DefencePlayResult {
        time_to_catch: ctx.time_to_catch,
        throw_target_base: throw_target.base,
        play_type: throw_target.play_type,
        final_fielder_position: final_fielder_position,
        cutoff_fielder_potition: throw_target.cutoff_fielder_position.clone(),
        defense_time: defense_play_time,
    };
    Ok(defence_play_result)
}

// TODO: case of final fielder failed to catch
fn outfield_defense_play(
    ctx: &PlayContext,
    throw_target: &ThrowTarget,
) -> Result<DefencePlayResult, GameError> {
    let distance_to_base = calculate_polar_distance(
        &ctx.ball.polar_position,
        &throw_target.base.polar_position(),
    );

    let defense_play_time = match throw_target.cutoff_fielder_position.clone() {
        Some(cutoff_position) => {
            let cutoff_fielder = find_fielder_by_position(ctx.fielders, cutoff_position)?;

            calculate_relay_play_time(
                ctx.try_catch_fielder,
                &ctx.ball.polar_position,
                throw_target.base,
                cutoff_fielder,
            )
        }
        None => {
            // Cutoff man not needed or not specified:
            // Calculate with direct throw time only
            let time_to_final_fielder = ctx.try_catch_fielder.prep_time
                + calculate_ball_flight_time(distance_to_base, ctx.try_catch_fielder.throw_speed);
            if throw_target.play_type == PlayType::ForcePlay {
                time_to_final_fielder
            } else {
                time_to_final_fielder + TOUCH_PENALTY_TIME
            }
        }
    };

    let defence_play_result = DefencePlayResult {
        time_to_catch: ctx.time_to_catch,
        throw_target_base: throw_target.base,
        play_type: throw_target.play_type,
        final_fielder_position: throw_target.final_fielder_position.clone(),
        cutoff_fielder_potition: throw_target.cutoff_fielder_position.clone(),
        defense_time: defense_play_time,
    };
    Ok(defence_play_result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayType {
    ForcePlay,
    TouchPlay,
}

#[derive(Debug)]
pub struct StealDefencePlayResult {
    pub throw_target_base: Base,
    pub final_fielder_position: Position,
    pub defense_time: f64,
}

#[derive(Debug)]
pub struct PitcherData {
    delivery_motion_time: f64,
}

#[derive(Debug)]
pub struct CatcherData {
    prep_time: f64,
    throw_speed: f64,
}

#[derive(Debug)]
pub struct PlayContext<'a> {
    pub runners: &'a RunnersOnBase,
    pub fielders: &'a [Fielder],
    pub try_catch_fielder: &'a Fielder,
    pub ball: &'a Ball,
    pub time_to_catch: f64,
    pub is_fly_catch: bool,
}

// CONSTRAINT: PlayType must be ForcePlay
#[derive(Debug)]
struct DoublePlayThrowTarget {
    from_base: Base,
    to_base: Base,
    thorower_fielder_position: Position,
    final_fielder_position: Position,
}

#[derive(Debug)]
struct ThrowTarget {
    base: Base,
    play_type: PlayType,
    final_fielder_position: Position,
    cutoff_fielder_position: Option<Position>,
}

fn judge_tagup_throw_target(ctx: &PlayContext) -> ThrowTarget {
    // Tag-up prevention strategy for no-bounce catches (fly/liner outs)
    // 1. Runner on third (4): top priority is to throw home (back home)
    if ctx.runners.has_runner_on(Base::Third) {
        // If the fly is too deep (e.g. 95m+), give up and throw to the infield (2nd base etc.)
        if ctx.ball.distance() <= DEEP_OUTFIELD_DISTANCE {
            return ThrowTarget {
                base: Base::Home,
                play_type: PlayType::TouchPlay,
                final_fielder_position: Position::C,
                cutoff_fielder_position: match ctx.try_catch_fielder.position {
                    Position::RF => Some(Position::SB),
                    _ => Some(Position::SS),
                },
            };
        }
    }

    // 2. Runner on second (2): prevent tagging up to third
    if ctx.runners.has_runner_on(Base::Second) {
        // Left-field fly: third baseman is either catching or off the base,
        // so only throw to third on center/right-field flies
        if matches!(ctx.try_catch_fielder.position, Position::CF | Position::RF) {
            return ThrowTarget {
                base: Base::Third,
                play_type: PlayType::TouchPlay,
                final_fielder_position: Position::TB,
                cutoff_fielder_position: Some(Position::SB),
            };
        }
    }

    // 3. No runner advancing, or the ball is too deep to throw
    // Throw cleanly back to the infield (second) to end the play
    let rng: f64 = rand::random();
    let catch_fielder = if rng < WEIGHT_SS_BASE_COVER {
        Position::SS
    } else {
        Position::SB
    };

    return ThrowTarget {
        base: Base::Second,
        play_type: PlayType::TouchPlay,
        final_fielder_position: catch_fielder,
        cutoff_fielder_position: None,
    };
}

fn judge_double_play_throw_target(
    // ctx: &PlayContext,
    defence_play_result: &DefencePlayResult,
) -> Option<DoublePlayThrowTarget> {
    let rng: f64 = rand::random();
    let second_base_cover_position = if rng < WEIGHT_SS_BASE_COVER {
        Position::SS
    } else {
        Position::SB
    };

    match defence_play_result.throw_target_base {
        Base::First => {
            if defence_play_result.final_fielder_position == Position::FB {
                return Some(DoublePlayThrowTarget {
                    from_base: Base::First,
                    to_base: Base::Second,
                    thorower_fielder_position: Position::FB,
                    final_fielder_position: second_base_cover_position,
                });
            } else {
                return None;
            }
        }
        Base::Second => {
            return Some(DoublePlayThrowTarget {
                from_base: Base::Second,
                to_base: Base::First,
                thorower_fielder_position: defence_play_result.final_fielder_position.clone(),
                final_fielder_position: Position::FB,
            });
        }
        Base::Third => {
            // CONSTRAINT: Throwing to second base is not covered
            return Some(DoublePlayThrowTarget {
                from_base: Base::Third,
                to_base: Base::First,
                thorower_fielder_position: defence_play_result.final_fielder_position.clone(), // Must be TB
                final_fielder_position: Position::FB,
            });
        }
        Base::Home => {
            let rng: f64 = rand::random();
            // CONSTRAINT: Effect of draw-in infield is not cosidered. Assuming TB is enough close to third base.
            let (target_base, final_fielder_position) = if rng < WEIGHT_IS_LOADED_TARGET_THIRD {
                (Base::Third, Position::TB)
            } else {
                (Base::First, Position::FB)
            };

            return Some(DoublePlayThrowTarget {
                from_base: Base::Home,
                to_base: target_base,
                thorower_fielder_position: defence_play_result.final_fielder_position.clone(), // Must be C
                final_fielder_position: final_fielder_position,
            });
        }
    }
}

fn judge_infield_grounder_throw_target(ctx: &PlayContext) -> ThrowTarget {
    // CONSTRAINT: Play back for the double play　 is not covered.
    if ctx.runners.is_loaded() {
        return ThrowTarget {
            base: Base::Home,
            play_type: PlayType::ForcePlay,
            final_fielder_position: Position::C,
            cutoff_fielder_position: None,
        };
    }

    // TODO: Consider the case of protecting the 1-point lead
    if ctx.runners.has_runner_on(Base::Third) && ctx.ball.distance() <= SHALLOW_INFIELD_DISTANCE {
        return ThrowTarget {
            base: Base::Home,
            play_type: PlayType::TouchPlay,
            final_fielder_position: Position::C,
            cutoff_fielder_position: None,
        };
    }

    // CONSTRAINT: Throwing to second base is not covered.
    if ctx.runners.has_first_and_second()
        && matches!(ctx.try_catch_fielder.position, Position::TB | Position::SS)
    {
        return ThrowTarget {
            base: Base::Third,
            play_type: PlayType::ForcePlay,
            final_fielder_position: Position::TB,
            cutoff_fielder_position: None,
        };
    }

    if ctx.runners.has_runner_on(Base::First) {
        if ctx.ball.distance() <= SHALLOW_INFIELD_DISTANCE
            && matches!(
                ctx.try_catch_fielder.position,
                Position::P | Position::C | Position::FB | Position::TB
            )
        {
            let catch_fielder = if ctx.try_catch_fielder.position == Position::FB {
                Position::P // Pitcher should first base cover
            } else {
                Position::FB
            };

            return ThrowTarget {
                base: Base::First,
                play_type: PlayType::ForcePlay,
                final_fielder_position: catch_fielder,
                cutoff_fielder_position: None,
            };
        }
        if matches!(
            ctx.try_catch_fielder.position,
            Position::SB | Position::SS | Position::P
        ) {
            let catch_fielder = if ctx.try_catch_fielder.position == Position::SB {
                Position::SS
            } else if ctx.try_catch_fielder.position == Position::SS {
                Position::SB
            } else {
                let rng: f64 = rand::random();
                if rng < WEIGHT_SS_BASE_COVER {
                    Position::SS
                } else {
                    Position::SB
                }
            };

            return ThrowTarget {
                base: Base::Second,
                play_type: PlayType::ForcePlay,
                final_fielder_position: catch_fielder,
                cutoff_fielder_position: None,
            };
        }
    }
    return ThrowTarget {
        base: Base::First,
        play_type: PlayType::ForcePlay,
        final_fielder_position: Position::FB,
        cutoff_fielder_position: None,
    };
}

fn judge_outfield_hit_throw_target(ctx: &PlayContext) -> ThrowTarget {
    // Outfield hit (extra-base hit or single)
    // 1. Runner on third (or second), and a throw home might arrive in time
    // Note: if time_to_catch is short and the outfielder is relatively shallow (within 80m), go for home
    if ctx.runners.has_runner_on(Base::Second) | ctx.runners.has_runner_on(Base::Third) {
        if ctx.ball.distance() <= CUTOFF_NEEDED_DISTACE_FOR_RUNNER_ON_THIRD
            && ctx.time_to_catch <= CUTOFF_NEEDED_TIME_TO_CATCH
        {
            // Determine cutoff man based on hit direction (fielder position)
            let cutoff = match ctx.try_catch_fielder.position {
                Position::RF => Some(Position::SB), // Right field: second base is cutoff
                _ => Some(Position::SS),            // Left/Center field: shortstop is cutoff
            };

            return ThrowTarget {
                base: Base::Home,
                play_type: PlayType::TouchPlay,
                final_fielder_position: Position::C,
                cutoff_fielder_position: cutoff,
            };
        }
    }

    // 2. Runner on first, want to prevent advancement to third (e.g. on a right-field single)
    if ctx.runners.has_runner_on(Base::First) {
        // Left-field hits often concede third, but right/center-field hits have a chance to nail them at third
        if matches!(ctx.try_catch_fielder.position, Position::CF | Position::RF)
            && ctx.ball.distance() <= CUTOFF_NEEDED_DISTACE_FOR_RUNNER_ON_FIRST
        {
            return ThrowTarget {
                base: Base::Third,
                play_type: PlayType::TouchPlay,
                final_fielder_position: Position::TB,
                cutoff_fielder_position: Some(Position::SB),
            };
        }
    }

    // 3. Extra-base hit (outfielder handling it near the fence at ~95m+)
    // Prevent the batter from advancing to second (or third)
    if ctx.ball.distance() >= DEEP_OUTFIELD_DISTANCE {
        // The fielder closer to the ball charges for the cutoff; the farther one covers the base
        let (cutoff, catch_fielder) = match ctx.try_catch_fielder.position {
            Position::LF => (Some(Position::SS), Position::SB), // Left field line: shortstop relays
            Position::RF => (Some(Position::SB), Position::SS), // Right field line: second base relays
            _ => (Some(Position::SS), Position::SB),            // Dead center: shortstop relays
        };

        return ThrowTarget {
            base: Base::Second,
            play_type: PlayType::TouchPlay,
            final_fielder_position: catch_fielder,
            cutoff_fielder_position: cutoff,
        };
    }

    // 4. Default (ordinary single, no aggressive base advancement expected)
    // Throw back to the infield to settle the play (conveniently use the nearest infield base)
    // For shallow hits, throw directly to second without a cutoff man
    let rng: f64 = rand::random();
    let catch_fielder = if rng < WEIGHT_SS_BASE_COVER {
        Position::SS
    } else {
        Position::SB
    };

    ThrowTarget {
        base: Base::Second,
        play_type: PlayType::TouchPlay,
        final_fielder_position: catch_fielder,
        cutoff_fielder_position: None,
    }
}

#[derive(Debug)]
pub struct DoublePlayDefencePlayResult {
    pub throw_target_base: Base,
    pub final_fielder_position: Position,
    pub defense_time: f64,
}

#[derive(Debug)]
pub struct DefencePlayResult {
    pub time_to_catch: f64,
    pub throw_target_base: Base,
    pub play_type: PlayType,
    pub final_fielder_position: Position,
    pub cutoff_fielder_potition: Option<Position>,
    pub defense_time: f64,
}

pub fn evaluate_base_stealing(
    target_base: Base,     // Second (steal 2nd) or Third (steal 3rd)
    pitcher: &PitcherData, // Quick motion speed, pitch velocity
    catcher: &CatcherData, // Arm strength (pop time), control
) -> StealDefencePlayResult {
    // 1. Defense side: total time from pitch to throw completion to 2nd (or 3rd)
    // Pitcher's motion time (1.0s for quick motion, ~1.3s for normal)
    let pitch_delivery_time = pitcher.delivery_motion_time;

    // Catcher's pop time (pro average is about 1.9–2.0s)
    // Varies with throw distance to the target base (2nd: ~38m, 3rd: ~27m)
    let home_pos = Base::Home.polar_position();
    let target_pos = target_base.polar_position();
    let throw_distance = calculate_polar_distance(&home_pos, &target_pos);

    let catcher_pop_time = catcher.prep_time + (throw_distance / catcher.throw_speed);

    let rng: f64 = rand::random();
    let final_fielder_position = if rng < WEIGHT_SS_BASE_COVER {
        Position::SS
    } else {
        Position::SB
    };

    // Total defense time = pitcher motion + catcher pop time + tag play (0.3s)
    let total_defense_time = pitch_delivery_time + catcher_pop_time + TOUCH_PENALTY_TIME;

    StealDefencePlayResult {
        throw_target_base: target_base,
        final_fielder_position: final_fielder_position,
        defense_time: total_defense_time,
    }
}

pub fn evaluate_double_play(
    ctx: &PlayContext,
    defence_play_result: &DefencePlayResult,
) -> Result<Option<DoublePlayDefencePlayResult>, GameError> {
    let double_play_throw_target = judge_double_play_throw_target(defence_play_result);
    if double_play_throw_target.is_none() {
        Ok(None)
    } else {
        if let Some(throw_target) = double_play_throw_target {
            let double_play_defence_play_result =
                double_play_defense_play(ctx.fielders, &throw_target)?;
            Ok(double_play_defence_play_result)
        } else {
            Ok(None)
        }
    }
}

// TODO: Consider double play by picking off
pub fn evaluate_defense_play(ctx: &PlayContext) -> Result<DefencePlayResult, GameError> {
    if ctx.ball.is_infield() {
        let throw_target = judge_infield_grounder_throw_target(ctx);
        let defence_play_result = infield_grounder_defense_play(ctx, &throw_target)?;
        Ok(defence_play_result)
    } else {
        let throw_target = if ctx.is_fly_catch {
            judge_tagup_throw_target(ctx)
        } else {
            judge_outfield_hit_throw_target(ctx)
        };

        let defence_play_result = outfield_defense_play(ctx, &throw_target)?;
        Ok(defence_play_result)
    }
}

#[derive(Debug)]
pub struct FieldingResult<'a> {
    pub is_fly_catch: bool,
    pub time_to_catch: f64,
    pub ball: &'a Ball,
}

#[derive(Debug)]
struct BoundedBallResult {
    time_to_fumble: f64, // Total time until the fielder picks up the ball
    final_distance: f64, // Final distance where the ball stopped (or hit the fence)
}

impl Fielder {
    pub fn new(
        position: Position,
        distance: f64,
        angle: f64,
        throw_speed: f64,
        running_speed: f64,
        reaction: f64,
        prep_time: f64,
    ) -> Self {
        Self {
            position: position,
            polar_position: PolarPosition::new(distance, angle),
            throw_speed: throw_speed,
            running_speed: running_speed,
            reaction: reaction,
            prep_time: prep_time,
        }
    }

    pub fn is(&self, position: Position) -> bool {
        self.position == position
    }

    pub fn distance(&self) -> f64 {
        self.polar_position.distance
    }

    pub fn angle(&self) -> f64 {
        self.polar_position.angle
    }

    pub fn x(&self) -> f64 {
        self.polar_position.x
    }

    pub fn y(&self) -> f64 {
        self.polar_position.y
    }

    pub fn try_catch<'a>(&self, ball: &'a mut Ball) -> FieldingResult<'a> {
        // $$\text{arrival\_time} = \text{reaction\_time} + \frac{\text{required\_distance}}{\text{fielder\_speed}}$$
        // 1. Calculate straight-line distance from position to landing point
        let required_distance =
            calculate_polar_distance(&self.polar_position, &ball.polar_position);
        let dy = self.y() - ball.y();

        // 3. Adjust initial reaction speed based on hit type (secret ingredient)
        let mut final_reaction = self.reaction;
        if ball.trajectory == TrajectoryType::Liner && dy < 0.0 {
            // Delay reaction when moving forward on a liner (harder to judge)
            final_reaction += LINER_REACTION_TIME;
        }

        // 4. Calculate arrival time (seconds)
        let arrival_time = final_reaction + (required_distance / self.running_speed);

        // 5. Compare arrival time vs hang time
        let (is_fly_catch, time_to_catch) = if ball.trajectory == TrajectoryType::Grounder {
            // Ruling delegates to evaluate_defense_play
            (false, arrival_time)
        } else if arrival_time <= ball.hang_time {
            (true, arrival_time)
        } else {
            let bounded_ball_result = self.process_bounded_ball(&ball);
            ball.polar_position.distance = bounded_ball_result.final_distance;
            (false, bounded_ball_result.time_to_fumble)
        };

        FieldingResult {
            is_fly_catch: is_fly_catch,
            time_to_catch: time_to_catch,
            ball: ball,
        }
    }

    // Processing when a fly/liner wasn't caught (became a hit)
    fn process_bounded_ball(&self, ball: &Ball) -> BoundedBallResult {
        // 1. Damping coefficient at the moment of the first bounce (liner bounces sharply, fly dies)
        let k_impact = match ball.trajectory {
            TrajectoryType::Liner => 0.60,
            TrajectoryType::Fly => 0.35,
            _ => 0.0,
        };

        // 2. Initial speed as a grounder right after the bounce
        let v_horizontal = ball.launch_speed_ms() * ball.azimuth().cos() * 0.7; // Velocity including in-flight air resistance
        let v_bounce = v_horizontal * k_impact;

        // 3. Additional rolling distance and time until stop
        let roll_distance = v_bounce * 1.8;

        // 4. Provisional final resting position (landing point + roll distance)
        let mut final_distance = ball.distance() + roll_distance;

        // The fence bounce (cushion) logic
        if final_distance > FENCE_DISTANCE {
            let overflow = final_distance - FENCE_DISTANCE;
            final_distance = FENCE_DISTANCE - (overflow * FENCE_BOUNCE_COEFF);
        }

        // 5. Defense: time for the fielder to chase down and pick up the rolling ball
        // The fielder was initially running toward the landing point but didn't make it.
        // Simple calculation of time to loop around toward the direction the ball rolled (final_distance)
        let fielder_distance_to_ball = (final_distance - self.distance()).abs();

        // Time for the fielder to reach the final resting point (or cushion treatment position)
        let fielder_arrival_time = self.reaction + (fielder_distance_to_ball / self.running_speed);

        // Time the fielder picks up the ball (either waiting for it to stop or cutting it off mid-roll)
        let time_to_pick_up = fielder_arrival_time.max(ball.hang_time + FIRST_BOUNCE_TIME);
        BoundedBallResult {
            final_distance,
            time_to_fumble: time_to_pick_up, // ★This becomes the time_to_catch for the next throw play!
        }
    }
}

pub fn find_closest_fielder<'f>(
    fielders: &'f [Fielder],
    ball: &Ball,
) -> Result<&'f Fielder, GameError> {
    // 1. Filter candidate fielders by whether the hit is infield or outfield
    let candidates: Vec<&Fielder> = fielders
        .iter()
        .filter(|f| {
            match ball.trajectory {
                // For grounders, infielders chase until the ball rolls past the infield
                TrajectoryType::Grounder => {
                    if ball.is_infield() {
                        // Infield grounder: only infielders (1B, 2B, 3B, SS) are candidates
                        matches!(
                            f.position,
                            Position::P
                                | Position::C
                                | Position::FB
                                | Position::SB
                                | Position::TB
                                | Position::SS
                        )
                    } else {
                        // Grounder through to the outfield: outfielders handle it
                        matches!(f.position, Position::LF | Position::CF | Position::RF)
                    }
                }
                // For fly balls and liners
                _ => {
                    if ball.is_shallow() {
                        // Shallow fly: both infielders and outfielders can chase
                        true
                    } else {
                        // Deep fly: only outfielders (LF, CF, RF) are candidates
                        matches!(f.position, Position::LF | Position::CF | Position::RF)
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

// Determine whether a fielder is in the ball's trajectory lane (lateral coverage)
fn is_ball_in_fielder_lane(fielder: &Fielder, ball_angle: f64) -> bool {
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

#[derive(Debug)]
pub struct FinalClosestFielder<'f, 'b> {
    pub fielder: &'f Fielder,
    pub ball: &'b Ball,
}

// Evaluate fielders on the trajectory lane from front to back (revised over-the-head version)
pub fn process_defensive_chain<'f, 'b>(
    fielders: &'f [Fielder],
    ball: &'b mut Ball,
) -> Result<FinalClosestFielder<'f, 'b>, GameError> {
    // 1. Sort fielders in the same lane by distance (closest first)
    let mut lane_fielders: Vec<&Fielder> = fielders
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
            fielder.reaction + (required_move_distance / fielder.running_speed);

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

            ball.hang_time = ball_arrival_time;

            return Ok(FinalClosestFielder {
                fielder: fielder,
                ball: ball,
            });
        }
    }

    // 3. Nobody touched it and it got through to the outfield (same as before: closest outfielder handles it)
    let final_closest = find_closest_fielder(fielders, ball)?;
    Ok(FinalClosestFielder {
        fielder: final_closest,
        ball: ball,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::resolver::running_resolver::Runner;
    use crate::domain::shared::player::RL;

    fn assert_near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {actual} to be near {expected}"
        );
    }

    fn fielder(position: Position, distance: f64, angle: f64) -> Fielder {
        Fielder::new(position, distance, angle, 35.0, 7.0, 0.4, 0.6)
    }

    fn ball(
        trajectory: TrajectoryType,
        distance: f64,
        angle: f64,
        hang_time: f64,
        launch_speed_kmh: f64,
        launch_angle: f64,
    ) -> Ball {
        Ball::new(
            launch_speed_kmh,
            launch_angle,
            angle,
            distance,
            hang_time,
            trajectory,
        )
    }

    fn runners_on_base(
        runner_1st_speed: Option<f64>,
        runner_2nd_speed: Option<f64>,
        runner_3rd_speed: Option<f64>,
    ) -> RunnersOnBase {
        fn runner(speed: f64) -> Runner {
            Runner {
                speed,
                lead_distance: 0.0,
                target_base: None,
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

    fn pitcher(delivery_motion_time: f64) -> PitcherData {
        PitcherData {
            delivery_motion_time,
        }
    }

    fn catcher(prep_time: f64, throw_speed: f64) -> CatcherData {
        CatcherData {
            prep_time,
            throw_speed,
        }
    }

    fn expected_steal_defense_time(
        target_base: Base,
        pitcher: &PitcherData,
        catcher: &CatcherData,
    ) -> f64 {
        let throw_distance =
            calculate_polar_distance(&Base::Home.polar_position(), &target_base.polar_position());

        pitcher.delivery_motion_time
            + catcher.prep_time
            + (throw_distance / catcher.throw_speed)
            + 0.3
    }

    fn default_fielders() -> [Fielder; 9] {
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
        let fielder = Fielder::new(Position::CF, 80.0, 0.0, 38.0, 7.5, 0.3, 0.5);

        assert_eq!(fielder.position, Position::CF);
        assert_near(fielder.distance(), 80.0);
        assert_near(fielder.angle(), 0.0);
        assert_near(fielder.x(), 0.0);
        assert_near(fielder.y(), 80.0);
        assert_near(fielder.throw_speed, 38.0);
        assert_near(fielder.running_speed, 7.5);
        assert_near(fielder.reaction, 0.3);
        assert_near(fielder.prep_time, 0.5);
    }

    #[test]
    fn calculate_cutoff_position_keeps_throw_line_and_uses_midpoint_weight() {
        let catch_position = PolarPosition::new(90.0, -20.0);

        let cutoff_position = calculate_cutoff_position(&catch_position, Base::Home);

        assert_near(cutoff_position.angle, catch_position.angle);
        assert_near(cutoff_position.distance, 90.0 * 0.45);
        assert_near(cutoff_position.x, 40.5 * (-20.0_f64).to_radians().sin());
        assert_near(cutoff_position.y, 40.5 * (-20.0_f64).to_radians().cos());
    }

    #[test]
    fn evaluate_base_stealing_to_second_returns_defense_time_and_covering_fielder() {
        let pitcher = pitcher(1.0);
        let catcher = catcher(0.4, 35.0);

        let result = evaluate_base_stealing(Base::Second, &pitcher, &catcher);

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
        let second_result = evaluate_base_stealing(Base::Second, &pitcher, &catcher);
        let third_result = evaluate_base_stealing(Base::Third, &pitcher, &catcher);

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
        let throw_distance = calculate_polar_distance(
            &Base::Second.polar_position(),
            &Base::First.polar_position(),
        );
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: thrower,
            ball: &ball,
            time_to_catch: 1.0,
            is_fly_catch: false,
        };
        let first_play = DefencePlayResult {
            time_to_catch: 1.0,
            throw_target_base: Base::Second,
            play_type: PlayType::ForcePlay,
            final_fielder_position: Position::SS,
            cutoff_fielder_potition: None,
            defense_time: 1.2,
        };

        let result = evaluate_double_play(&ctx, &first_play).unwrap().unwrap();
        let expected_defense_time =
            thrower.prep_time + calculate_ball_flight_time(throw_distance, thrower.throw_speed);

        assert_eq!(result.throw_target_base, Base::First);
        assert_eq!(result.final_fielder_position, Position::FB);
        assert_near(result.defense_time, expected_defense_time);
    }

    #[test]
    fn evaluate_double_play_returns_error_when_last_fielder_is_missing() {
        let fielders = [fielder(Position::SS, 35.0, -33.0)];
        let runners = runners_on_base(Some(7.0), None, None);
        let ball = ball(TrajectoryType::Grounder, 35.0, -25.0, 1.0, 95.0, 4.0);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &fielders[0],
            ball: &ball,
            time_to_catch: 1.0,
            is_fly_catch: false,
        };
        let first_play = DefencePlayResult {
            time_to_catch: 1.0,
            throw_target_base: Base::Second,
            play_type: PlayType::ForcePlay,
            final_fielder_position: Position::SS,
            cutoff_fielder_potition: None,
            defense_time: 1.2,
        };

        let result = evaluate_double_play(&ctx, &first_play);

        assert!(
            matches!(result, Err(GameError::NoPlayerFor(position)) if position == Position::FB.to_string())
        );
    }

    #[test]
    fn try_catch_returns_out_when_fielder_arrives_before_airborne_ball_lands() {
        let center_fielder = fielder(Position::CF, 75.0, 0.0);
        let mut fly_ball = ball(TrajectoryType::Fly, 80.0, 0.0, 3.0, 120.0, 35.0);

        let result = center_fielder.try_catch(&mut fly_ball);

        assert!(result.is_fly_catch);
        assert_near(result.time_to_catch, 0.4 + (5.0 / 7.0));
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
        assert!(result.time_to_catch > original_hang_time);
        assert!(result.ball.distance() > original_distance);
        assert_near(result.ball.angle(), 0.0);
    }

    #[test]
    fn try_catch_treats_grounders_as_safe_for_later_base_race() {
        let shortstop = fielder(Position::SS, 30.0, -5.0);
        let mut grounder = ball(TrajectoryType::Grounder, 32.0, -5.0, 0.9, 95.0, 4.0);

        let result = shortstop.try_catch(&mut grounder);

        assert!(!result.is_fly_catch);
        assert_near(result.time_to_catch, 0.4 + (2.0 / 7.0));
        assert_near(result.ball.distance(), 32.0);
        assert_near(result.ball.angle(), -5.0);
    }

    #[test]
    fn liner_beyond_fielder_adds_reaction_delay() {
        let center_fielder = fielder(Position::CF, 80.0, 0.0);
        let mut liner_beyond_fielder = ball(TrajectoryType::Liner, 90.0, 0.0, 2.0, 110.0, 15.0);

        let result = center_fielder.try_catch(&mut liner_beyond_fielder);

        assert!(result.is_fly_catch);
        assert_near(result.time_to_catch, 0.55 + (10.0 / 7.0));
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
        let mut grounder = grounder;
        let expected_arrival_time = grounder.hang_time * (28.0 / 90.0);

        let result = process_defensive_chain(&fielders, &mut grounder).unwrap();

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
        let mut fly_ball = ball(TrajectoryType::Fly, 85.0, 0.0, 3.5, 130.0, 35.0);
        let expected_arrival_time = fly_ball.hang_time * (80.0 / 85.0);

        let result = process_defensive_chain(&fielders, &mut fly_ball).unwrap();

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
        let mut grounder = ball(TrajectoryType::Grounder, 80.0, 25.0, 2.0, 90.0, 5.0);
        let original_hang_time = grounder.hang_time;

        let result = process_defensive_chain(&fielders, &mut grounder).unwrap();

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
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &pitcher,
            ball: &grounder,
            time_to_catch: 0.8,
            is_fly_catch: false,
        };

        let target = judge_infield_grounder_throw_target(&ctx);

        assert_target(target, Base::Home, PlayType::ForcePlay);
    }

    #[test]
    fn judge_optimal_target_general_chooses_third_force_for_left_side_grounder() {
        let third_baseman = fielder(Position::TB, 32.0, -35.0);
        let grounder = ball(TrajectoryType::Grounder, 30.0, -35.0, 1.0, 95.0, 4.0);
        let fielders = default_fielders();
        let runners = runners_on_base(Some(7.0), Some(7.0), None);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &third_baseman,
            ball: &grounder,
            time_to_catch: 1.0,
            is_fly_catch: false,
        };

        let target = judge_infield_grounder_throw_target(&ctx);

        assert_target(target, Base::Third, PlayType::ForcePlay);
    }

    #[test]
    fn judge_optimal_target_general_throws_home_on_shallow_hit_with_lead_runner() {
        let center_fielder = fielder(Position::CF, 75.0, 0.0);
        let shallow_hit = ball(TrajectoryType::Liner, 70.0, 0.0, 3.0, 100.0, 15.0);
        let fielders = default_fielders();
        let runners = runners_on_base(None, None, Some(7.0));
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &center_fielder,
            ball: &shallow_hit,
            time_to_catch: 3.0,
            is_fly_catch: false,
        };

        let target = judge_outfield_hit_throw_target(&ctx);

        assert_target(target, Base::Home, PlayType::TouchPlay);
    }

    #[test]
    fn judge_optimal_target_general_chooses_third_on_shallow_center_hit_with_runner_on_first() {
        let center_fielder = fielder(Position::CF, 75.0, 0.0);
        let shallow_hit = ball(TrajectoryType::Liner, 72.0, 5.0, 3.8, 100.0, 15.0);
        let fielders = default_fielders();
        let runners = runners_on_base(Some(7.0), None, None);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &center_fielder,
            ball: &shallow_hit,
            time_to_catch: 3.8,
            is_fly_catch: false,
        };

        let target = judge_outfield_hit_throw_target(&ctx);

        assert_target(target, Base::Third, PlayType::TouchPlay);
    }

    #[test]
    fn judge_optimal_target_general_chooses_second_for_deep_extra_base_hit() {
        let left_fielder = fielder(Position::LF, 90.0, -25.0);
        let deep_hit = ball(TrajectoryType::Fly, 95.0, -20.0, 5.0, 120.0, 35.0);
        let fielders = default_fielders();
        let runners = runners_on_base(None, None, None);
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &left_fielder,
            ball: &deep_hit,
            time_to_catch: 5.0,
            is_fly_catch: false,
        };

        let target = judge_outfield_hit_throw_target(&ctx);

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
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &first_baseman,
            ball: &grounder,
            time_to_catch: 0.8,
            is_fly_catch: false,
        };

        let result = evaluate_defense_play(&ctx).unwrap();
        let throw_distance =
            calculate_polar_distance(&catch_position, &Base::First.polar_position());
        let expected_defense_time = (first_baseman.prep_time
            + (throw_distance / first_baseman.throw_speed))
            .min(throw_distance / first_baseman.running_speed);

        assert_eq!(result.time_to_catch, 0.8);
        assert_eq!(result.throw_target_base, Base::First);
        assert_eq!(result.play_type, PlayType::ForcePlay);
        assert_eq!(result.final_fielder_position, Position::FB);
        assert_eq!(result.cutoff_fielder_potition, None);
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
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &left_fielder,
            ball: &shallow_hit,
            time_to_catch: 3.2,
            is_fly_catch: true,
        };

        let result = evaluate_defense_play(&ctx).unwrap();
        let shortstop = fielders
            .iter()
            .find(|fielder| fielder.is(Position::SS))
            .unwrap();
        let expected_defense_time =
            calculate_relay_play_time(&left_fielder, &catch_position, Base::Home, shortstop);

        assert_eq!(result.time_to_catch, 3.2);
        assert_eq!(result.throw_target_base, Base::Home);
        assert_eq!(result.play_type, PlayType::TouchPlay);
        assert_eq!(result.final_fielder_position, Position::C);
        assert_eq!(result.cutoff_fielder_potition, Some(Position::SS));
        assert_near(result.defense_time, expected_defense_time);
    }

    #[test]
    fn evaluate_defense_play_handles_deep_outfield_hit_to_second() {
        let center_fielder = fielder(Position::CF, 90.0, 0.0);
        let deep_hit = ball(TrajectoryType::Fly, 95.0, 0.0, 5.0, 120.0, 35.0);
        let runners = runners_on_base(None, None, None);
        let fielders = default_fielders();
        let ctx = PlayContext {
            runners: &runners,
            fielders: &fielders,
            try_catch_fielder: &center_fielder,
            ball: &deep_hit,
            time_to_catch: 5.0,
            is_fly_catch: false,
        };

        let result = evaluate_defense_play(&ctx).unwrap();
        let shortstop = fielders
            .iter()
            .find(|fielder| fielder.is(Position::SS))
            .unwrap();
        let expected_defense_time = calculate_relay_play_time(
            &center_fielder,
            &deep_hit.polar_position,
            Base::Second,
            shortstop,
        );

        assert_eq!(result.time_to_catch, 5.0);
        assert_eq!(result.throw_target_base, Base::Second);
        assert_eq!(result.play_type, PlayType::TouchPlay);
        assert_eq!(result.final_fielder_position, Position::SB);
        assert_eq!(result.cutoff_fielder_potition, Some(Position::SS));
        assert_near(result.defense_time, expected_defense_time);
    }
}
