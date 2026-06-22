use crate::domain::shared::ball::{Ball, TrajectoryType};
use crate::domain::shared::game::BASE_DISTANCE;
use crate::domain::shared::game_state::{GameError, Ruling};
use crate::domain::shared::player::{Position, RL};
use crate::domain::shared::stadium::Base;
use crate::domain::util::{PolarPosition, calculate_distance};

// TODO: fence distance should be retrieved from the stadium
const FENCE_DISTANCE: f64 = 100.0; // Stadium fence distance (assumed 100m)
const FENCE_BOUNCE_COEFF: f64 = 0.25; // Fence bounce coefficient (grounder cushion is quite damped)
const LINER_REACTION_TIME: f64 = 0.15; // TODO: should be moved to Player ability

// Bitmask representing runner state on bases (takes values 0–7)
// Example: runners on first and third = 1 + 4 = 5 (101)
pub const RUNNER_NONE: u8 = 0; // No runners (000)
pub const RUNNER_1ST: u8 = 1; // Runner on first (001)
pub const RUNNER_2ND: u8 = 2; // Runner on second (010)
pub const RUNNER_3RD: u8 = 4; // Runner on third (100)
pub const RUNNER_FULL: u8 = 7; // Runner on first and second and third (111)
pub const RUNNER_1ST_AND_2ND: u8 = 3; // Runner on first and second (011)

// Calculate actual ball flight time based on distance
fn calculate_ball_flight_time(distance: f64, initial_throw_speed: f64) -> f64 {
    // Base distance: assume top speed can be maintained up to 30m
    if distance <= 30.0 {
        return distance / initial_throw_speed;
    }

    // Beyond 30m, add a mild delay proportional to distance squared (air resistance penalty)
    // A 100m direct throw takes roughly 0.5–0.8s longer than the simple calculation
    let base_time = distance / initial_throw_speed;
    let penalty_factor = 1.0 + (distance - 30.0).powi(2) * 0.0001;

    base_time * penalty_factor
}

// Determine the optimal cutoff man position based on catch position and target base
fn calculate_cutoff_position(catch_pos: &PolarPosition, target_base: Base) -> PolarPosition {
    let base_pos = target_base.polar_position();

    // Keep the angle aligned with the outfielder's direction (to stay on the direct line)
    let cutoff_angle = catch_pos.angle;

    // Place the cutoff man at a good relay point between the target base and the outfield
    // Example: outfield at 90m, base at 0m (home) → place cutoff around 35–40m
    let target_r = base_pos.distance;
    let cutoff_distance = target_r + (catch_pos.distance - target_r) * 0.45;

    PolarPosition::new(cutoff_distance, cutoff_angle)
}

fn calculate_relay_play_time(
    fielder: &Fielder,
    catch_pos: &PolarPosition,
    target_base: Base,
    cutoff: &Fielder, // Infielder data for the cutoff relay
) -> f64 {
    let base_pos = target_base.polar_position();

    // --- Pattern A: Direct throw ---
    let direct_dist = calculate_distance(&catch_pos, &base_pos);
    let direct_flight_time = calculate_ball_flight_time(direct_dist, fielder.throw_speed);
    let total_direct_time = fielder.prep_time + direct_flight_time;

    // --- Pattern B: Cutoff relay play ---
    let cutoff_pos = calculate_cutoff_position(catch_pos, target_base);

    // Outfielder → cutoff man throw time
    let dist_1st = calculate_distance(&catch_pos, &cutoff_pos);
    let flight_time_1st = calculate_ball_flight_time(dist_1st, fielder.throw_speed);

    // Cutoff man → base throw time
    let dist_2nd = calculate_distance(&catch_pos, &base_pos);
    let flight_time_2nd = calculate_ball_flight_time(dist_2nd, cutoff.throw_speed);

    // Total relay time = outfielder prep + 1st flight + cutoff prep + 2nd flight
    let total_relay_time = fielder.prep_time + flight_time_1st + cutoff.prep_time + flight_time_2nd;

    // Pick the better option
    if total_relay_time < total_direct_time {
        total_relay_time // Cutoff route is faster (ball doesn't slow as much)
    } else {
        total_direct_time // Direct throw is faster (shallow fly, tag-up situations, etc.)
    }
}

// Running speed of each runner on base (None if no runner)
#[derive(Clone, Copy, Debug)]
pub struct RunnersOnBase {
    pub batter_speed: f64, // Batter always exists
    pub runner_1st_speed: Option<f64>,
    pub runner_2nd_speed: Option<f64>,
    pub runner_3rd_speed: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayType {
    ForcePlay,
    TouchPlay,
}

#[derive(Debug)]
pub struct StealResult {
    ruling: Ruling,
    defense_time: f64,
    runner_time: f64,
    updated_runners: RunnersOnBase,
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

// Dedicated function for stolen base judgment
pub fn evaluate_base_stealing(
    target_base: Base,     // Second (steal 2nd) or Third (steal 3rd)
    pitcher: &PitcherData, // Quick motion speed, pitch velocity
    catcher: &CatcherData, // Arm strength (pop time), control
    runner_speed: f64,     // Runner's pure running speed (m/s)
    lead_distance: f64,    // Lead distance (m)
    start_reaction: f64,   // Start reaction time (seconds; 0.1 for good jump, 0.6 if picked off)
    current_runners: &RunnersOnBase,
) -> StealResult {
    // 1. Defense side: total time from pitch to throw completion to 2nd (or 3rd)
    // Pitcher's motion time (1.0s for quick motion, ~1.3s for normal)
    let pitch_delivery_time = pitcher.delivery_motion_time;

    // Catcher's pop time (pro average is about 1.9–2.0s)
    // Varies with throw distance to the target base (2nd: ~38m, 3rd: ~27m)
    let home_pos = Base::Home.polar_position();
    let target_pos = target_base.polar_position();
    let throw_distance = calculate_distance(&home_pos, &target_pos);

    let catcher_pop_time = catcher.prep_time + (throw_distance / catcher.throw_speed);

    // Total defense time = pitcher motion + catcher pop time + tag play (0.3s)
    let total_defense_time = pitch_delivery_time + catcher_pop_time + 0.3;

    // 2. Runner side: total time to slide into the next base
    let running_distance = BASE_DISTANCE - lead_distance;

    // Runner total time = reaction delay + actual running time
    let total_runner_time = start_reaction + (running_distance / runner_speed);

    // 3. Outcome judgment and runner state update
    let ruling = if total_defense_time <= total_runner_time {
        Ruling::Out
    } else {
        Ruling::Safe
    };

    let mut next_1st = current_runners.runner_1st_speed;
    let mut next_2nd = current_runners.runner_2nd_speed;
    let mut next_3rd = current_runners.runner_3rd_speed;

    if ruling == Ruling::Out {
        // [Out]: the runner who attempted to advance is removed
        match target_base {
            Base::Second => next_1st = None,
            Base::Third => next_2nd = None,
            _ => {}
        }
    } else {
        // [Safe]: the runner successfully advances
        match target_base {
            Base::Second => {
                next_2nd = current_runners.runner_1st_speed;
                next_1st = None;
            }
            Base::Third => {
                next_3rd = current_runners.runner_2nd_speed;
                next_2nd = None;
            }
            _ => {}
        }
    }

    StealResult {
        ruling: ruling,
        defense_time: total_defense_time,
        runner_time: total_runner_time,
        updated_runners: RunnersOnBase {
            batter_speed: current_runners.batter_speed,
            runner_1st_speed: next_1st,
            runner_2nd_speed: next_2nd,
            runner_3rd_speed: next_3rd,
        },
    }
}

#[derive(Debug)]
pub struct PlayContext<'a> {
    pub bases_occupied: u8,
    pub fielder: &'a Fielder,
    pub ball: &'a Ball,
    pub time_to_catch: f64, // Time taken to catch (or process the hit)
    pub is_fly_catch: bool,
}

#[derive(Debug)]
struct AutoTarget {
    base: Base,
    play_type: PlayType,
    cutoff_fielder: Option<Position>, // TODO: Replace Position to Player
}

// Automatically determine the optimal target base based on base state and catching fielder's position
fn judge_optimal_target_general(ctx: &PlayContext) -> AutoTarget {
    //---------------------------------------------------------
    // C. Tag-up prevention strategy for no-bounce catches (fly/liner outs)
    //---------------------------------------------------------
    if ctx.is_fly_catch {
        // 1. Runner on third (4): top priority is to throw home (back home)
        if (ctx.bases_occupied & RUNNER_3RD) == RUNNER_3RD {
            // If the fly is too deep (e.g. 95m+), give up and throw to the infield (2nd base etc.)
            if ctx.ball.distance() <= 90.0 {
                return AutoTarget {
                    base: Base::Home,
                    play_type: PlayType::TouchPlay,
                    cutoff_fielder: match ctx.fielder.position {
                        Position::RF => Some(Position::SB),
                        _ => Some(Position::SS),
                    },
                };
            }
        }

        // 2. Runner on second (2): prevent tagging up to third
        if (ctx.bases_occupied & RUNNER_2ND) == RUNNER_2ND {
            // Left-field fly: third baseman is either catching or off the base,
            // so only throw to third on center/right-field flies
            if matches!(ctx.fielder.position, Position::CF | Position::RF) {
                return AutoTarget {
                    base: Base::Third,
                    play_type: PlayType::TouchPlay,
                    cutoff_fielder: Some(Position::SB),
                };
            }
        }

        // 3. No runner advancing, or the ball is too deep to throw
        // Throw cleanly back to the infield (second) to end the play
        return AutoTarget {
            base: Base::Second,
            play_type: PlayType::TouchPlay,
            cutoff_fielder: None,
        };
    }

    // A. Infield grounder (situation where an out can be made) → previous logic
    if ctx.ball.is_infield() {
        if ctx.bases_occupied == RUNNER_FULL {
            return AutoTarget {
                base: Base::Home,
                play_type: PlayType::ForcePlay,
                cutoff_fielder: None,
            };
        }

        // TODO: Consider the case of protecting the 1-point lead
        if (ctx.bases_occupied & RUNNER_3RD) == RUNNER_3RD && ctx.ball.distance() <= 25.0 {
            return AutoTarget {
                base: Base::Home,
                play_type: PlayType::TouchPlay,
                cutoff_fielder: None,
            };
        }
        if (ctx.bases_occupied & RUNNER_1ST_AND_2ND) == RUNNER_1ST_AND_2ND
            && matches!(ctx.fielder.position, Position::TB | Position::SS)
        {
            return AutoTarget {
                base: Base::Third,
                play_type: PlayType::ForcePlay,
                cutoff_fielder: None,
            };
        }
        if (ctx.bases_occupied & RUNNER_1ST) == RUNNER_1ST {
            if ctx.ball.distance() <= 25.0
                && matches!(
                    ctx.fielder.position,
                    Position::P | Position::C | Position::FB | Position::TB
                )
            {
                return AutoTarget {
                    base: Base::First,
                    play_type: PlayType::ForcePlay,
                    cutoff_fielder: None,
                };
            }
            if matches!(
                ctx.fielder.position,
                Position::SB | Position::SS | Position::P
            ) {
                return AutoTarget {
                    base: Base::Second,
                    play_type: PlayType::ForcePlay,
                    cutoff_fielder: None,
                };
            }
        }
        return AutoTarget {
            base: Base::First,
            play_type: PlayType::ForcePlay,
            cutoff_fielder: None,
        };
    }

    // B. Outfield hit (extra-base hit or single) → generalized extension
    // 1. Runner on third (or second), and a throw home might arrive in time
    // Note: if time_to_catch is short and the outfielder is relatively shallow (within 80m), go for home
    if (ctx.bases_occupied & (RUNNER_2ND | RUNNER_3RD)) != 0 {
        if ctx.ball.distance() <= 80.0 && ctx.time_to_catch <= 3.5 {
            // Determine cutoff man based on hit direction (fielder position)
            let cutoff = match ctx.fielder.position {
                Position::RF => Some(Position::SB), // Right field: second base is cutoff
                _ => Some(Position::SS),            // Left/Center field: shortstop is cutoff
            };

            return AutoTarget {
                base: Base::Home,
                play_type: PlayType::TouchPlay,
                cutoff_fielder: cutoff,
            };
        }
    }

    // 2. Runner on first, want to prevent advancement to third (e.g. on a right-field single)
    if (ctx.bases_occupied & RUNNER_1ST) == RUNNER_1ST {
        // Left-field hits often concede third, but right/center-field hits have a chance to nail them at third
        if matches!(ctx.fielder.position, Position::CF | Position::RF)
            && ctx.ball.distance() <= 75.0
        {
            return AutoTarget {
                base: Base::Third,
                play_type: PlayType::TouchPlay,
                cutoff_fielder: Some(Position::SB),
            };
        }
    }

    // 3. Extra-base hit (outfielder handling it near the fence at ~95m+)
    // Prevent the batter from advancing to second (or third)
    if ctx.ball.distance() >= 90.0 {
        // The fielder closer to the ball charges for the cutoff; the farther one covers the base
        let cutoff = match ctx.fielder.position {
            Position::LF => Some(Position::SS), // Left field line: shortstop relays
            Position::RF => Some(Position::SB), // Right field line: second base relays
            _ => Some(Position::SS),            // Dead center: shortstop relays
        };
        return AutoTarget {
            base: Base::Second,
            play_type: PlayType::TouchPlay,
            cutoff_fielder: cutoff,
        };
    }

    // 4. Default (ordinary single, no aggressive base advancement expected)
    // Throw back to the infield to settle the play (conveniently use the nearest infield base)
    // For shallow hits, throw directly to second without a cutoff man
    AutoTarget {
        base: Base::Second,
        play_type: PlayType::TouchPlay,
        cutoff_fielder: None,
    }
}

#[derive(Debug)]
pub struct PlayResult {
    pub ruling: Ruling,
    pub defense_time: f64,
    pub runner_time: f64,
    pub time_difference: f64, // For determining if it's a close play
    pub updated_runners: RunnersOnBase, // Runner state after the play completes
    pub runs_scored: u16,     // Runs scored on this play (0–3)
    pub target_base: Base,
    pub next_throw_position: Option<Position>,
}

pub fn evaluate_double_play(
    fielders: &[Fielder],
    runners: &RunnersOnBase,
    batting_side: RL,
    target_base: Base,
    last_fielder_position: Position,
) -> Result<PlayResult, GameError> {
    // 1. Determine defense target based on the passed target
    let base_pos = target_base.polar_position();

    // The second throw target is always first base (Base::First)
    let first_base_pos = Base::First.polar_position();

    // 2. Time race calculation (throw/run vs target runner)
    let distance_to_base = calculate_distance(&base_pos, &first_base_pos);

    let thrower = fielders
        .iter()
        .find(|i| i.is(last_fielder_position.clone()))
        .map(|i| i)
        .ok_or_else(|| GameError::NoPlayerFor(last_fielder_position.to_string()))?;

    let defense_play_time =
        thrower.prep_time + calculate_ball_flight_time(distance_to_base, thrower.throw_speed) + 0.3;

    let (running_distance, target_runner_speed) = {
        let dist = match batting_side {
            RL::Right => BASE_DISTANCE + 2.0,
            RL::Left => BASE_DISTANCE,
        };
        (dist, Some(runners.batter_speed)) // Batter always exists
    };

    let total_runner_time = match target_runner_speed {
        Some(speed) => {
            // When the target is preventing extra-base advancement (e.g. runner on 1st going to 3rd, runner on 2nd heading home),
            // the runner is already through the previous base and accelerating, so reduce the acceleration penalty slightly
            let acceleration_lag = 0.2;
            (running_distance / speed) + acceleration_lag
        }
        None => {
            // [Important] If a throw is made to a base with no runner (logic error or poor decision)
            // Set runner time to 0s, forcing the defense to be Safe (failure) as a safety guard
            0.0
        }
    };

    let ruling = if defense_play_time <= total_runner_time {
        Ruling::Out
    } else {
        Ruling::Safe
    };
    let time_difference = (defense_play_time - total_runner_time).abs();

    let mut next_1st = Some(runners.batter_speed);
    let next_2nd = runners.runner_1st_speed;
    let next_3rd = runners.runner_2nd_speed;
    let mut runs_scored: u16 = 0;

    if runners.runner_3rd_speed.is_some() {
        runs_scored += 1;
    }

    if ruling == Ruling::Out {
        next_1st = None;
    }

    let updated_runners = RunnersOnBase {
        batter_speed: runners.batter_speed, // Kept for the next batter (effectively reset)
        runner_1st_speed: next_1st,
        runner_2nd_speed: next_2nd,
        runner_3rd_speed: next_3rd,
    };

    Ok(PlayResult {
        ruling: ruling,
        defense_time: defense_play_time,
        runner_time: total_runner_time,
        time_difference: time_difference,
        updated_runners: updated_runners,
        runs_scored: runs_scored,
        target_base: target_base,
        next_throw_position: None,
    })
}

fn evaluate_tagup_play(
    ctx: &PlayContext,
    fielders: &[Fielder],
    runners: &RunnersOnBase,
) -> Result<PlayResult, GameError> {
    // Get the tag-up specific target (home, third, etc.)
    // Base advance is 0 since it's a fly out
    let target = judge_optimal_target_general(&ctx);
    let base_pos = target.base.polar_position();

    // 1. Calculate total defense time (based on t=0)
    let distance_to_base = calculate_distance(&ctx.ball.polar_position, &base_pos);

    let defense_play_time = match target.cutoff_fielder {
        Some(cutoff_position) => {
            let cutoff_man = fielders
                .iter()
                .find(|i| i.is(cutoff_position.clone()))
                .map(|i| i)
                .ok_or_else(|| GameError::NoPlayerFor(cutoff_position.to_string()));

            calculate_relay_play_time(
                ctx.fielder,
                &ctx.ball.polar_position,
                target.base,
                cutoff_man?,
            )
        }
        None => {
            // Cutoff man not needed or not specified:
            // Calculate with direct throw (or self-run) time only
            ctx.fielder.prep_time
                + calculate_ball_flight_time(distance_to_base, ctx.fielder.throw_speed)
                + 0.3
        }
    };
    let final_defense_time = ctx.time_to_catch + defense_play_time;

    // 3. Calculate total runner time (based on t=0)
    // Extract the running speed of the runner based on the target base
    let runner_speed = match target.base {
        Base::Home => runners.runner_3rd_speed, // Third base runner coming home
        Base::Third => runners.runner_2nd_speed, // Second base runner tagging to third
        _ => None, // First base tagging to second is essentially a non-play (None)
    };

    let total_runner_time = match runner_speed {
        Some(speed) => {
            // Runner starts from the moment of the catch, so add acceleration lag (0.5s) from a standing start
            ctx.time_to_catch + (BASE_DISTANCE / speed) + 0.5
        }
        None => 0.0, // No runner, or the runner didn't attempt due to defensive alignment
    };

    let ruling = if final_defense_time <= total_runner_time && total_runner_time > 0.0 {
        Ruling::Out
    } else {
        Ruling::Safe
    };

    // 4. Update runner state and calculate runs scored (tag-up specific)
    let next_1st = runners.runner_1st_speed;
    let mut next_2nd = runners.runner_2nd_speed;
    let mut next_3rd = runners.runner_3rd_speed;
    let mut runs_scored = 0;

    // Initial state shift when a runner attempts to advance
    match target.base {
        Base::Home => {
            next_3rd = None; // Third base runner has left the base
            if ruling == Ruling::Safe {
                runs_scored += 1;
            } // If safe, run scores
        }
        Base::Third => {
            next_2nd = None; // Second base runner has left the base
            if ruling == Ruling::Safe {
                next_3rd = runners.runner_2nd_speed;
            } // If safe, runner holds third
        }
        _ => {} // If no one ran, the original base state is preserved
    }

    Ok(PlayResult {
        ruling: ruling,
        defense_time: final_defense_time,
        runner_time: total_runner_time,
        time_difference: (final_defense_time - total_runner_time).abs(),
        updated_runners: RunnersOnBase {
            batter_speed: runners.batter_speed, // Batter is already out; only kept for the next batter
            runner_1st_speed: next_1st,
            runner_2nd_speed: next_2nd,
            runner_3rd_speed: next_3rd,
        },
        runs_scored: runs_scored,
        target_base: target.base,
        next_throw_position: None,
    })
}

// TODO: Return who's the Cut Off Man
fn evaluate_outfield_hit_play(
    ctx: &PlayContext,
    fielders: &[Fielder],
    runners: &RunnersOnBase,
    lead_distance: f64, // Should be set 0 in case Tag Up
    batting_side: RL,   // Batter's side; only used for batter-runner distance adjustment
) -> Result<PlayResult, GameError> {
    // 1. Dynamically determine how many bases the batter can advance (base_advance)
    let base_advance: u8;

    // Calculate the time for the batter to reach each base
    let dist_1st = match batting_side {
        RL::Right => BASE_DISTANCE + 2.0,
        RL::Left => BASE_DISTANCE,
    };
    // TODO: 0.5 should be replaced by runner's abiliry or sign
    let t1 = (dist_1st / runners.batter_speed) + 0.5;
    let t2 = ((dist_1st + BASE_DISTANCE) / runners.batter_speed) + 0.5;
    // t3 may be used for running home run
    // let t3 = ((dist_1st + BASE_DISTANCE * 2.0) / runners.batter_speed) + 0.5;

    // At the moment the fielder handles the ball, how far has the batter advanced?
    // The slower the fielder's processing (larger time_to_catch), the more bases the batter can take
    if ctx.time_to_catch > t2 - 0.5 {
        base_advance = 3; // Triple
    } else if ctx.time_to_catch > t1 - 0.5 {
        base_advance = 2; // Double
    } else {
        base_advance = 1; // Single
    }

    // 2. Determine defense target based on the dynamically calculated base_advance
    // Automatically decide which base to target based on baseball theory
    // TODO: Change to pass base_advance to judge_optimal_target_general
    let target = judge_optimal_target_general(ctx);
    let base_pos = target.base.polar_position();

    // 3. Time race calculation (throw/run vs target runner)
    let distance_to_base = calculate_distance(&ctx.ball.polar_position, &base_pos);

    let defense_play_time = match target.cutoff_fielder {
        Some(cutoff_position) => {
            let cutoff_man = fielders
                .iter()
                .find(|i| i.is(cutoff_position.clone()))
                .map(|i| i)
                .ok_or_else(|| GameError::NoPlayerFor(cutoff_position.to_string()));

            calculate_relay_play_time(
                ctx.fielder,
                &ctx.ball.polar_position,
                target.base,
                cutoff_man?,
            )
        }
        None => {
            // Cutoff man not needed or not specified:
            // Calculate with direct throw (or self-run) time only
            if target.play_type == PlayType::ForcePlay {
                let time_via_throw = ctx.fielder.prep_time
                    + calculate_ball_flight_time(distance_to_base, ctx.fielder.throw_speed);
                let time_via_run = distance_to_base / ctx.fielder.running_speed;
                time_via_run.min(time_via_throw)
            } else {
                ctx.fielder.prep_time + (distance_to_base / ctx.fielder.throw_speed) + 0.3
            }
        }
    };
    let final_defense_time = ctx.time_to_catch + defense_play_time;

    // 4. Dynamically extract runner's distance and speed for the target (Option-aware)
    // Unwrap the Option<f64> for the target base via pattern matching
    let (running_distance, target_runner_speed): (f64, Option<f64>) = match target.base {
        Base::First => {
            let dist = match batting_side {
                RL::Right => BASE_DISTANCE + 2.0,
                RL::Left => BASE_DISTANCE,
            };
            (dist, Some(runners.batter_speed)) // Batter always exists
        }
        Base::Second => (
            (BASE_DISTANCE - lead_distance).max(0.0),
            runners.runner_1st_speed,
        ),
        Base::Third => (
            (BASE_DISTANCE - lead_distance).max(0.0),
            runners.runner_2nd_speed.or(runners.runner_1st_speed),
        ),
        Base::Home => (
            (BASE_DISTANCE - lead_distance).max(0.0),
            runners
                .runner_3rd_speed
                .or(runners.runner_2nd_speed)
                .or(runners.runner_1st_speed),
        ),
    };

    // 5. Calculate runner time with safety guard logic
    let total_runner_time = match target_runner_speed {
        Some(speed) => {
            // When the target is preventing extra-base advancement (e.g. runner on 1st going to 3rd, runner on 2nd heading home),
            // the runner is already through the previous base and accelerating, so reduce the acceleration penalty slightly
            let acceleration_lag = if target.base != Base::First { 0.2 } else { 0.5 };
            (running_distance / speed) + acceleration_lag
        }
        None => {
            // [Important] If a throw is made to a base with no runner (logic error or poor decision)
            // Set runner time to 0s, forcing the defense to be Safe (failure) as a safety guard
            0.0
        }
    };

    // 6. Determine the outcome
    // When runner_time is 0.0 (None case), total_defense_time > 0.0 always holds,
    // so is_out = false (Safe / fielder's choice), preventing a game crash.
    let ruling = if final_defense_time <= total_runner_time {
        Ruling::Out
    } else {
        Ruling::Safe
    };
    let time_difference = (final_defense_time - total_runner_time).abs();

    // 7. [Fully automatic] Update runner state and calculate runs scored
    let mut next_1st = None;
    let mut next_2nd = None;
    let mut next_3rd = None;
    let mut runs_scored: u16 = 0;

    // Shift runners based on the dynamically calculated base_advance
    match base_advance {
        1 => {
            // TODO: Consider the case 1st runner goes to 3rd base
            next_1st = Some(runners.batter_speed);
            next_2nd = runners.runner_1st_speed;
            next_3rd = runners.runner_2nd_speed;
            if runners.runner_3rd_speed.is_some() {
                runs_scored += 1;
            }
        }
        2 => {
            next_2nd = Some(runners.batter_speed);
            next_3rd = runners.runner_1st_speed;
            if runners.runner_2nd_speed.is_some() {
                runs_scored += 1;
            }
            if runners.runner_3rd_speed.is_some() {
                runs_scored += 1;
            }
        }
        3 => {
            next_3rd = Some(runners.batter_speed);
            if runners.runner_1st_speed.is_some() {
                runs_scored += 1;
            }
            if runners.runner_2nd_speed.is_some() {
                runs_scored += 1;
            }
            if runners.runner_3rd_speed.is_some() {
                runs_scored += 1;
            }
        }
        _ => {}
    }

    // Overwrite: remove the runner who was put out
    if ruling == Ruling::Out {
        match target.base {
            Base::First => next_1st = None,
            Base::Second => next_2nd = None,
            Base::Third => next_3rd = None,
            Base::Home => {
                next_3rd = None;
                runs_scored = runs_scored.saturating_sub(1);
            }
        }
    }

    // Build the new runner state struct
    let updated_runners = RunnersOnBase {
        batter_speed: runners.batter_speed, // Kept for the next batter (effectively reset)
        runner_1st_speed: next_1st,
        runner_2nd_speed: next_2nd,
        runner_3rd_speed: next_3rd,
    };

    Ok(PlayResult {
        ruling: ruling,
        defense_time: final_defense_time,
        runner_time: total_runner_time,
        time_difference: time_difference,
        updated_runners: updated_runners,
        runs_scored: runs_scored,
        target_base: target.base,
        next_throw_position: None,
    })
}

// TODO: Return who's the Cut Off Man
pub fn evaluate_grounder_play(
    ctx: &PlayContext,
    runners: &RunnersOnBase,
    lead_distance: f64,
    batting_side: RL, // Batter's side; only used for batter-runner distance adjustment
) -> Result<PlayResult, GameError> {
    // 1. Determine defense target based on the dynamically calculated base_advance
    // Automatically decide which base to target based on baseball theory
    // TODO: Change to pass base_advance to judge_optimal_target_general
    let target = judge_optimal_target_general(ctx);
    let base_pos = target.base.polar_position();

    // 2. Time race calculation (throw/run vs target runner)
    let distance_to_base = calculate_distance(&ctx.ball.polar_position, &base_pos);

    // Decide who catch the last ball
    let last_fielder_position = match target.base {
        Base::First => Position::FB,
        Base::Second => {
            if ctx.fielder.position == Position::SB {
                Position::SS
            } else {
                Position::SB
            }
        }
        Base::Third => Position::TB,
        Base::Home => Position::C,
    };

    // Cutoff man not needed or not specified:
    // Calculate with direct throw (or self-run) time only
    let defense_play_time = if target.play_type == PlayType::ForcePlay {
        let time_via_throw = ctx.fielder.prep_time
            + calculate_ball_flight_time(distance_to_base, ctx.fielder.throw_speed);
        let time_via_run = distance_to_base / ctx.fielder.running_speed;
        time_via_run.min(time_via_throw)
    } else {
        ctx.fielder.prep_time
            + calculate_ball_flight_time(distance_to_base, ctx.fielder.throw_speed)
            + 0.3
    };

    let final_defense_time = ctx.time_to_catch + defense_play_time;

    // 3. Dynamically extract runner's distance and speed for the target (Option-aware)
    // Unwrap the Option<f64> for the target base via pattern matching
    let (running_distance, target_runner_speed): (f64, Option<f64>) = match target.base {
        Base::First => {
            let dist = match batting_side {
                RL::Right => BASE_DISTANCE + 2.0,
                RL::Left => BASE_DISTANCE,
            };
            (dist, Some(runners.batter_speed)) // Batter always exists
        }
        Base::Second => (
            (BASE_DISTANCE - lead_distance).max(0.0),
            runners.runner_1st_speed,
        ),
        Base::Third => (
            (BASE_DISTANCE - lead_distance).max(0.0),
            runners.runner_2nd_speed.or(runners.runner_1st_speed),
        ),
        Base::Home => (
            (BASE_DISTANCE - lead_distance).max(0.0),
            runners
                .runner_3rd_speed
                .or(runners.runner_2nd_speed)
                .or(runners.runner_1st_speed),
        ),
    };

    // 4. Calculate runner time with safety guard logic
    let total_runner_time = match target_runner_speed {
        Some(speed) => {
            // When the target is preventing extra-base advancement (e.g. runner on 1st going to 3rd, runner on 2nd heading home),
            // the runner is already through the previous base and accelerating, so reduce the acceleration penalty slightly
            let acceleration_lag = if target.base != Base::First { 0.2 } else { 0.5 };
            (running_distance / speed) + acceleration_lag
        }
        None => {
            // [Important] If a throw is made to a base with no runner (logic error or poor decision)
            // Set runner time to 0s, forcing the defense to be Safe (failure) as a safety guard
            0.0
        }
    };

    // 5. Determine the outcome
    // When runner_time is 0.0 (None case), total_defense_time > 0.0 always holds,
    // so is_out = false (Safe / fielder's choice), preventing a game crash.
    let ruling = if final_defense_time <= total_runner_time {
        Ruling::Out
    } else {
        Ruling::Safe
    };
    let time_difference = (final_defense_time - total_runner_time).abs();

    // 5. [Fully automatic] Update runner state and calculate runs scored
    let mut next_1st = Some(runners.batter_speed);
    let mut next_2nd = runners.runner_1st_speed;
    let mut next_3rd = runners.runner_2nd_speed;
    let mut runs_scored: u16 = 0;

    if runners.runner_3rd_speed.is_some() {
        runs_scored += 1;
    }

    // Overwrite: remove the runner who was put out
    if ruling == Ruling::Out {
        match target.base {
            Base::First => next_1st = None,
            Base::Second => next_2nd = None,
            Base::Third => next_3rd = None,
            Base::Home => {
                next_3rd = None;
                runs_scored = runs_scored.saturating_sub(1);
            }
        }
    }

    // Build the new runner state struct
    let updated_runners = RunnersOnBase {
        batter_speed: runners.batter_speed, // Kept for the next batter (effectively reset)
        runner_1st_speed: next_1st,
        runner_2nd_speed: next_2nd,
        runner_3rd_speed: next_3rd,
    };

    Ok(PlayResult {
        ruling: ruling,
        defense_time: final_defense_time,
        runner_time: total_runner_time,
        time_difference: time_difference,
        updated_runners: updated_runners,
        runs_scored: runs_scored,
        target_base: target.base,
        next_throw_position: Some(last_fielder_position),
    })
}

// TODO: Return who's the Cut Off Man
pub fn evaluate_defense_play(
    ctx: &PlayContext,
    fielders: &[Fielder],
    runners: &RunnersOnBase,
    lead_distance: f64,
    batting_side: RL, // Batter's side; only used for batter-runner distance adjustment
) -> Result<PlayResult, GameError> {
    if ctx.is_fly_catch {
        // 1. Delegate to tag-up judgment for fly/liner outs
        evaluate_tagup_play(ctx, fielders, runners)
    } else if !ctx.ball.is_infield() {
        // 2. Delegate to advance/extra-base-hit prevention for outfield hits (to be implemented)
        evaluate_outfield_hit_play(ctx, fielders, runners, lead_distance, batting_side)
    } else {
        // 3. Delegate to force play/self-run judgment for infield grounders (previous base logic)
        evaluate_grounder_play(ctx, runners, lead_distance, batting_side)
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

// TODO: merge into Player
#[derive(Debug, Clone)]
pub struct Fielder {
    pub position: Position,
    pub polar_position: PolarPosition,
    pub throw_speed: f64,   // Throw speed (m/s) e.g. 35.0 – 42.0 m/s
    pub running_speed: f64, // Running speed (m/s) e.g. 6.5 – 8.0 m/s
    pub reaction: f64,      // Reaction time (seconds) e.g. 0.3 – 0.7 s (lower is better)
    pub prep_time: f64, // Pitch preparation / transfer time (seconds) e.g. 0.5 – 0.8 s (lower is better)
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
        let required_distance = calculate_distance(&self.polar_position, &ball.polar_position);
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
        let time_to_pick_up = fielder_arrival_time.max(ball.hang_time + 0.5); // At least 0.5s after the first bounce

        BoundedBallResult {
            final_distance,
            time_to_fumble: time_to_pick_up, // ★This becomes the time_to_catch for the next throw play!
        }
    }
}

pub fn find_closest_fielder<'f>(fielders: &'f [Fielder], ball: &Ball) -> &'f Fielder {
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
            let dist_a = calculate_distance(&a.polar_position, &ball.polar_position);
            let dist_b = calculate_distance(&b.polar_position, &ball.polar_position);

            // Use partial_cmp safely since f64 is not a total order
            dist_a
                .partial_cmp(&dist_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(&fielders[1])
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
) -> FinalClosestFielder<'f, 'b> {
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

            // Maximum jump catch height for a fielder (2.5m)
            // TODO: Should be changed to Player's ability
            let max_reach_height = 2.5;

            match ball.trajectory {
                // Case 1 & 2: Fly, pop-up, or high liner
                TrajectoryType::Fly | TrajectoryType::PopUp | TrajectoryType::Liner => {
                    if ball_height > max_reach_height {
                        // Angle and timing are right, but it's too high even for a jump catch!
                        continue;
                    }
                }
                // Grounders always have height 0 and never go over the head (proceed to catch)
                TrajectoryType::Grounder => {}
            }

            ball.hang_time = ball_arrival_time;

            return FinalClosestFielder {
                fielder: fielder,
                ball: ball,
            };
        }
    }

    // 3. Nobody touched it and it got through to the outfield (same as before: closest outfielder handles it)
    let final_closest = find_closest_fielder(fielders, ball);
    FinalClosestFielder {
        fielder: final_closest,
        ball: ball,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        RunnersOnBase {
            batter_speed: 7.0,
            runner_1st_speed,
            runner_2nd_speed,
            runner_3rd_speed,
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
            calculate_distance(&Base::Home.polar_position(), &target_base.polar_position());

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

    fn assert_target(target: AutoTarget, expected_base: Base, expected_play_type: PlayType) {
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
    fn evaluate_base_stealing_moves_first_runner_to_second_when_safe() {
        let pitcher = pitcher(1.0);
        let catcher = catcher(0.4, 35.0);
        let runners = runners_on_base(Some(9.5), None, Some(6.8));

        let result =
            evaluate_base_stealing(Base::Second, &pitcher, &catcher, 9.5, 4.0, 0.1, &runners);

        assert!(result.ruling == Ruling::Safe);
        assert_near(
            result.defense_time,
            expected_steal_defense_time(Base::Second, &pitcher, &catcher),
        );
        assert_near(result.runner_time, 0.1 + ((27.431 - 4.0) / 9.5));
        assert_eq!(result.updated_runners.runner_1st_speed, None);
        assert_eq!(result.updated_runners.runner_2nd_speed, Some(9.5));
        assert_eq!(result.updated_runners.runner_3rd_speed, Some(6.8));
        assert_near(result.updated_runners.batter_speed, runners.batter_speed);
    }

    #[test]
    fn evaluate_base_stealing_removes_first_runner_when_caught_stealing_second() {
        let pitcher = pitcher(1.0);
        let catcher = catcher(0.4, 35.0);
        let runners = runners_on_base(Some(6.0), None, Some(6.8));

        let result =
            evaluate_base_stealing(Base::Second, &pitcher, &catcher, 6.0, 1.0, 0.6, &runners);

        assert!(result.ruling == Ruling::Out);
        assert!(result.defense_time <= result.runner_time);
        assert_near(
            result.defense_time,
            expected_steal_defense_time(Base::Second, &pitcher, &catcher),
        );
        assert_near(result.runner_time, 0.6 + ((27.431 - 1.0) / 6.0));
        assert_eq!(result.updated_runners.runner_1st_speed, None);
        assert_eq!(result.updated_runners.runner_2nd_speed, None);
        assert_eq!(result.updated_runners.runner_3rd_speed, Some(6.8));
    }

    #[test]
    fn evaluate_base_stealing_moves_second_runner_to_third_when_safe() {
        let pitcher = pitcher(1.0);
        let catcher = catcher(0.4, 35.0);
        let runners = runners_on_base(Some(7.0), Some(8.5), None);

        let result =
            evaluate_base_stealing(Base::Third, &pitcher, &catcher, 8.5, 7.0, 0.05, &runners);

        assert!(result.ruling == Ruling::Safe);
        assert_near(
            result.defense_time,
            expected_steal_defense_time(Base::Third, &pitcher, &catcher),
        );
        assert_near(result.runner_time, 0.05 + ((27.431 - 7.0) / 8.5));
        assert_eq!(result.updated_runners.runner_1st_speed, Some(7.0));
        assert_eq!(result.updated_runners.runner_2nd_speed, None);
        assert_eq!(result.updated_runners.runner_3rd_speed, Some(8.5));
    }

    #[test]
    fn evaluate_base_stealing_removes_second_runner_when_caught_stealing_third() {
        let pitcher = pitcher(1.0);
        let catcher = catcher(0.4, 35.0);
        let runners = runners_on_base(Some(7.0), Some(6.0), None);

        let result =
            evaluate_base_stealing(Base::Third, &pitcher, &catcher, 6.0, 1.0, 0.5, &runners);

        assert!(result.ruling == Ruling::Out);
        assert!(result.defense_time <= result.runner_time);
        assert_near(
            result.defense_time,
            expected_steal_defense_time(Base::Third, &pitcher, &catcher),
        );
        assert_near(result.runner_time, 0.5 + ((27.431 - 1.0) / 6.0));
        assert_eq!(result.updated_runners.runner_1st_speed, Some(7.0));
        assert_eq!(result.updated_runners.runner_2nd_speed, None);
        assert_eq!(result.updated_runners.runner_3rd_speed, None);
    }

    #[test]
    fn evaluate_double_play_records_out_when_second_throw_beats_batter_runner() {
        let fielders = default_fielders();
        let runners = RunnersOnBase {
            batter_speed: 7.0,
            runner_1st_speed: Some(6.5),
            runner_2nd_speed: None,
            runner_3rd_speed: Some(6.8),
        };
        let thrower = fielders
            .iter()
            .find(|fielder| fielder.is(Position::SS))
            .unwrap();
        let throw_distance = calculate_distance(
            &Base::Second.polar_position(),
            &Base::First.polar_position(),
        );

        let result =
            evaluate_double_play(&fielders, &runners, RL::Right, Base::Second, Position::SS)
                .unwrap();
        let expected_defense_time = thrower.prep_time
            + calculate_ball_flight_time(throw_distance, thrower.throw_speed)
            + 0.3;
        let expected_runner_time = ((BASE_DISTANCE + 2.0) / runners.batter_speed) + 0.2;

        assert_eq!(result.ruling, Ruling::Out);
        assert_near(result.defense_time, expected_defense_time);
        assert_near(result.runner_time, expected_runner_time);
        assert_near(
            result.time_difference,
            (result.defense_time - result.runner_time).abs(),
        );
        assert_eq!(result.updated_runners.runner_1st_speed, None);
        assert_eq!(result.updated_runners.runner_2nd_speed, Some(6.5));
        assert_eq!(result.updated_runners.runner_3rd_speed, None);
        assert_eq!(result.runs_scored, 1);
        assert_eq!(result.target_base, Base::Second);
        assert_eq!(result.next_throw_position, None);
    }

    #[test]
    fn evaluate_double_play_keeps_batter_on_first_when_second_throw_loses_race() {
        let slow_thrower = Fielder::new(Position::SB, 40.0, 18.0, 5.0, 7.0, 0.4, 0.6);
        let fielders = [slow_thrower.clone()];
        let runners = RunnersOnBase {
            batter_speed: 9.0,
            runner_1st_speed: Some(7.2),
            runner_2nd_speed: Some(6.9),
            runner_3rd_speed: None,
        };
        let throw_distance = calculate_distance(
            &Base::Second.polar_position(),
            &Base::First.polar_position(),
        );

        let result =
            evaluate_double_play(&fielders, &runners, RL::Left, Base::Second, Position::SB)
                .unwrap();
        let expected_defense_time = slow_thrower.prep_time
            + calculate_ball_flight_time(throw_distance, slow_thrower.throw_speed)
            + 0.3;
        let expected_runner_time = (BASE_DISTANCE / runners.batter_speed) + 0.2;

        assert_eq!(result.ruling, Ruling::Safe);
        assert_near(result.defense_time, expected_defense_time);
        assert_near(result.runner_time, expected_runner_time);
        assert_eq!(result.updated_runners.runner_1st_speed, Some(9.0));
        assert_eq!(result.updated_runners.runner_2nd_speed, Some(7.2));
        assert_eq!(result.updated_runners.runner_3rd_speed, Some(6.9));
        assert_eq!(result.runs_scored, 0);
    }

    #[test]
    fn evaluate_double_play_gives_right_handed_batter_extra_running_distance() {
        let fielders = default_fielders();
        let runners = runners_on_base(Some(7.0), None, None);

        let left_result =
            evaluate_double_play(&fielders, &runners, RL::Left, Base::Second, Position::SS)
                .unwrap();
        let right_result =
            evaluate_double_play(&fielders, &runners, RL::Right, Base::Second, Position::SS)
                .unwrap();

        assert_near(
            left_result.runner_time,
            (BASE_DISTANCE / runners.batter_speed) + 0.2,
        );
        assert_near(
            right_result.runner_time,
            ((BASE_DISTANCE + 2.0) / runners.batter_speed) + 0.2,
        );
        assert_near(
            right_result.runner_time - left_result.runner_time,
            2.0 / runners.batter_speed,
        );
    }

    #[test]
    fn evaluate_double_play_returns_error_when_last_fielder_is_missing() {
        let fielders = [fielder(Position::SS, 35.0, -33.0)];
        let runners = runners_on_base(Some(7.0), None, None);

        let result =
            evaluate_double_play(&fielders, &runners, RL::Left, Base::Second, Position::SB);

        assert!(
            matches!(result, Err(GameError::NoPlayerFor(position)) if position == Position::SB.to_string())
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

        let closest = find_closest_fielder(&fielders, &grounder);

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

        let closest = find_closest_fielder(&fielders, &fly_ball);

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

        let result = process_defensive_chain(&fielders, &mut grounder);

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

        let result = process_defensive_chain(&fielders, &mut fly_ball);

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

        let result = process_defensive_chain(&fielders, &mut grounder);

        assert_eq!(result.fielder.position, Position::CF);
        assert_eq!(result.ball.trajectory, TrajectoryType::Grounder);
        assert_near(result.ball.hang_time, original_hang_time);
    }

    #[test]
    fn judge_optimal_target_general_chooses_home_force_with_bases_loaded_grounder() {
        let pitcher = fielder(Position::P, 18.0, 0.0);
        let grounder = ball(TrajectoryType::Grounder, 18.0, 0.0, 0.8, 95.0, 4.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_FULL,
            fielder: &pitcher,
            ball: &grounder,
            time_to_catch: 0.8,
            is_fly_catch: false,
        };

        let target = judge_optimal_target_general(&ctx);

        assert_target(target, Base::Home, PlayType::ForcePlay);
    }

    #[test]
    fn judge_optimal_target_general_chooses_third_force_for_left_side_grounder() {
        let third_baseman = fielder(Position::TB, 32.0, -35.0);
        let grounder = ball(TrajectoryType::Grounder, 30.0, -35.0, 1.0, 95.0, 4.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_1ST_AND_2ND,
            fielder: &third_baseman,
            ball: &grounder,
            time_to_catch: 1.0,
            is_fly_catch: false,
        };

        let target = judge_optimal_target_general(&ctx);

        assert_target(target, Base::Third, PlayType::ForcePlay);
    }

    #[test]
    fn judge_optimal_target_general_throws_home_on_shallow_hit_with_lead_runner() {
        let center_fielder = fielder(Position::CF, 75.0, 0.0);
        let shallow_hit = ball(TrajectoryType::Liner, 70.0, 0.0, 3.0, 100.0, 15.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_3RD,
            fielder: &center_fielder,
            ball: &shallow_hit,
            time_to_catch: 3.0,
            is_fly_catch: false,
        };

        let target = judge_optimal_target_general(&ctx);

        assert_target(target, Base::Home, PlayType::TouchPlay);
    }

    #[test]
    fn judge_optimal_target_general_chooses_third_on_shallow_center_hit_with_runner_on_first() {
        let center_fielder = fielder(Position::CF, 75.0, 0.0);
        let shallow_hit = ball(TrajectoryType::Liner, 72.0, 5.0, 3.8, 100.0, 15.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_1ST,
            fielder: &center_fielder,
            ball: &shallow_hit,
            time_to_catch: 3.8,
            is_fly_catch: false,
        };

        let target = judge_optimal_target_general(&ctx);

        assert_target(target, Base::Third, PlayType::TouchPlay);
    }

    #[test]
    fn judge_optimal_target_general_chooses_second_for_deep_extra_base_hit() {
        let left_fielder = fielder(Position::LF, 90.0, -25.0);
        let deep_hit = ball(TrajectoryType::Fly, 95.0, -20.0, 5.0, 120.0, 35.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_NONE,
            fielder: &left_fielder,
            ball: &deep_hit,
            time_to_catch: 5.0,
            is_fly_catch: false,
        };

        let target = judge_optimal_target_general(&ctx);

        assert_target(target, Base::Second, PlayType::TouchPlay);
    }

    #[test]
    fn evaluate_grounder_play_records_out_when_force_play_beats_batter_runner() {
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
        let ctx = PlayContext {
            bases_occupied: RUNNER_NONE,
            fielder: &first_baseman,
            ball: &grounder,
            time_to_catch: 0.8,
            is_fly_catch: false,
        };
        let runners = runners_on_base(None, None, None);

        let result = evaluate_grounder_play(&ctx, &runners, 0.0, RL::Left).unwrap();
        let throw_distance = calculate_distance(&catch_position, &Base::First.polar_position());
        let expected_defense_time = 0.8
            + (first_baseman.prep_time + (throw_distance / first_baseman.throw_speed))
                .min(throw_distance / first_baseman.running_speed);
        let expected_runner_time = BASE_DISTANCE / runners.batter_speed + 0.5;

        assert_eq!(result.ruling, Ruling::Out);
        assert!(result.defense_time < result.runner_time);
        assert_near(result.defense_time, expected_defense_time);
        assert_near(result.runner_time, expected_runner_time);
        assert_near(
            result.time_difference,
            (result.defense_time - result.runner_time).abs(),
        );
    }

    #[test]
    fn evaluate_tagup_play_records_safe_when_tag_play_loses_home_race() {
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
        let ctx = PlayContext {
            bases_occupied: RUNNER_3RD,
            fielder: &left_fielder,
            ball: &shallow_hit,
            time_to_catch: 3.2,
            is_fly_catch: true,
        };
        let runners = runners_on_base(None, None, Some(10.0));
        let fielders = default_fielders();

        let result = evaluate_tagup_play(&ctx, &fielders, &runners).unwrap();
        let shortstop = fielders
            .iter()
            .find(|fielder| fielder.is(Position::SS))
            .unwrap();
        let throw_time =
            calculate_relay_play_time(&left_fielder, &catch_position, Base::Home, shortstop);
        let expected_defense_time = ctx.time_to_catch + throw_time;
        let expected_runner_time = ctx.time_to_catch + (BASE_DISTANCE / 10.0) + 0.5;

        assert_eq!(result.ruling, Ruling::Safe);
        assert!(result.defense_time > result.runner_time);
        assert_near(result.defense_time, expected_defense_time);
        assert_near(result.runner_time, expected_runner_time);
    }

    #[test]
    fn evaluate_outfield_hit_play_returns_safe_when_selected_target_has_no_runner() {
        let center_fielder = fielder(Position::CF, 90.0, 0.0);
        let deep_hit = ball(TrajectoryType::Fly, 95.0, 0.0, 5.0, 120.0, 35.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_NONE,
            fielder: &center_fielder,
            ball: &deep_hit,
            time_to_catch: 5.0,
            is_fly_catch: false,
        };
        let runners = runners_on_base(None, None, None);
        let fielders = default_fielders();

        let result = evaluate_outfield_hit_play(&ctx, &fielders, &runners, 0.0, RL::Left).unwrap();

        assert_eq!(result.ruling, Ruling::Safe);
        assert_near(result.runner_time, 0.0);
        assert!(result.defense_time > 0.0);
    }
}
