use crate::domain::resolver::batting_resolver::Runner;
use crate::domain::shared::ball::{Ball, TrajectoryType};
use crate::domain::shared::game_state::Ruling;
use crate::domain::shared::player::{Position, RL};
use crate::domain::shared::stadium::Base;
use crate::domain::util::{PolarPosition, calculate_distance};

// TODO: fence distance should be retrieved the stadium
const FENCE_DISTANCE: f64 = 100.0; // Stadium fence distance (assumed 100m)
const FENCE_BOUNCE_COEFF: f64 = 0.25; // Fence bounce coefficient (grounder cushion is quite damped)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayType {
    ForcePlay,
    TouchPlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefenseAction {
    Throw,     // Threw the ball
    SelfTouch, // Stepped on the base themselves
}

struct PlayResult {
    ruling: Ruling,
    defense_time: f64,
    runner_time: f64,
    time_difference: f64,  // For determining if it's a close play
    action: DefenseAction, // Action recorded for text commentary branching
}

fn evaluate_throw_play(
    time_to_catch: f64,
    fielder: &Fielder, // Fielder who caught the ball (arm strength, transfer speed)
    catch_pos: PolarPosition, // Coordinates where the grounder was caught (Polar)
    target_base: Base, // Target base for the throw
    play_type: PlayType, // Force play or tag play
    runner: &Runner,   // Runner (running speed, lead distance)
    batting_side: RL,  // Batter's side; only used for batter-runner distance adjustment
) -> PlayResult {
    // 1. Calculate total defense time (defense_total_time)
    let base_pos = target_base.polar_position();

    // Common distance from catch position to target base
    let distance_to_base = calculate_distance(&catch_pos, &base_pos);

    // 2. Ball flight time = distance / throw speed (m/s)
    let ball_flight_time = distance_to_base / fielder.throw_speed;

    // Add action penalty (0.3s) for touch play
    let touch_time = match play_type {
        PlayType::ForcePlay => 0.0,
        PlayType::TouchPlay => 0.3, // Delay for plays requiring a tag
    };

    let time_via_throw = fielder.prep_time + ball_flight_time + touch_time;

    // 3. Calculate time when running to the base themselves (prep time is 0)
    let time_via_run = distance_to_base / fielder.running_speed;

    // 4. Automatically select the shorter (better) action
    let (defense_play_time, action) = if time_via_run < time_via_throw {
        (time_via_run, DefenseAction::SelfTouch)
    } else {
        (time_via_throw, DefenseAction::Throw)
    };

    // 5. Total defense time from the moment of contact (t=0)
    let total_defense_time = time_to_catch + defense_play_time;

    // 6. Total runner time (based on t=0)
    // Get the remaining distance the runner needs to cover
    let initial_running_distance = runner.get_running_distance(batting_side);
    let total_runner_time = (initial_running_distance / runner.speed) + 0.5;

    // 7. Determine the outcome
    let ruling = if total_defense_time <= total_runner_time {
        Ruling::Out
    } else {
        Ruling::Safe
    };
    let time_difference = (total_defense_time - total_runner_time).abs();

    PlayResult {
        ruling,
        defense_time: total_defense_time,
        runner_time: total_runner_time,
        time_difference,
        action,
    }
}

#[derive(Debug)]
pub struct FieldingResult {
    pub ruling: Ruling,
    pub time_to_catch: f64,
    pub final_distance: f64,
}

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

    pub fn try_catch(&self, ball: &Ball) -> FieldingResult {
        // $$\text{arrival\_time} = \text{reaction\_time} + \frac{\text{required\_distance}}{\text{fielder\_speed}}$$
        // 1. Calculate straight-line distance from position to landing point
        let required_distance = calculate_distance(&self.polar_position, &ball.polar_position);
        let dy = self.y() - ball.y();

        // 3. Adjust initial reaction speed based on hit type (secret ingredient)
        let mut final_reaction = self.reaction;
        if ball.trajectory == TrajectoryType::Liner && dy < 0.0 {
            // Delay reaction when moving forward on a liner (harder to judge)
            final_reaction += 0.15;
        }

        // 4. Calculate arrival time (seconds)
        let arrival_time = final_reaction + (required_distance / self.running_speed);

        // 5. Compare arrival time vs hang time
        let (ruling, time_to_catch, final_distance) = if ball.trajectory == TrajectoryType::Grounder
        {
            // Ruling delegates to time race
            (Ruling::Safe, arrival_time, ball.distance())
        } else if arrival_time <= ball.hang_time {
            (Ruling::Out, arrival_time, ball.distance())
        } else {
            let bounded_ball_result = self.process_bounded_ball(&ball);
            (
                Ruling::Safe,
                bounded_ball_result.time_to_fumble,
                bounded_ball_result.final_distance,
            )
        };

        FieldingResult {
            ruling: ruling,
            time_to_catch: time_to_catch,
            final_distance: final_distance,
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
        let roll_time = if v_bounce > 0.0 {
            roll_distance / (v_bounce * 0.5)
        } else {
            0.0
        };

        // 4. Provisional final resting position (landing point + roll distance)
        let mut final_distance = ball.distance() + roll_distance;

        // The fence bounce (cushion) logic
        let mut total_roll_time = roll_time;
        if final_distance > FENCE_DISTANCE {
            let overflow = final_distance - FENCE_DISTANCE;
            final_distance = FENCE_DISTANCE - (overflow * 0.25);
            // If it hits the fence, rolling time stops there
            total_roll_time *= (FENCE_DISTANCE - ball.distance()) / roll_distance;
        }

        // 5. Defense: time for the fielder to chase down and pick up the rolling ball
        // The fielder was initially running toward the landing point but didn't make it.
        // Simple calculation of time to loop around toward the direction the ball rolled (final_distance)
        let ball_stop_time = ball.hang_time + total_roll_time; // Time when the ball stops

        // Time for the fielder to reach the final resting point (or cushion treatment position)
        let base_pos = Base::Home.polar_position(); // Simplified: same straight-line calculation
        let fielder_distance_to_ball = (final_distance - self.distance()).abs();
        let fielder_arrival_time = self.reaction + (fielder_distance_to_ball / self.running_speed);

        // Time the fielder picks up the ball (either waiting for it to stop or cutting it off mid-roll)
        let time_to_pick_up = fielder_arrival_time.max(ball.hang_time + 0.5); // At least 0.5s after the first bounce

        BoundedBallResult {
            final_distance,
            time_to_fumble: time_to_pick_up, // ★This becomes the time_to_catch for the next throw play!
        }
    }
}

pub fn find_closest_fielder(fielders: &[Fielder], ball: &Ball) -> Fielder {
    // 1. Filter candidate fielders by whether the hit is infield or outfield
    let candidates: Vec<&Fielder> = fielders
        .iter()
        .filter(|f| {
            match ball.trajectory {
                // For grounders, infielders chase until the ball rolls past the infield
                TrajectoryType::Grounder => {
                    if ball.distance() < 50.0 {
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
                    if ball.distance() < 45.0 {
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
        .clone() // Safety net: return second base as fallback if candidates are empty
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

pub struct FinalClosestFielder {
    pub fielder: Fielder,
    pub ball_arrival_time: f64,
    pub is_fly_catch: bool,
}

// Evaluate fielders on the trajectory lane from front to back (revised over-the-head version)
fn process_defensive_chain(fielders: &[Fielder], ball: &Ball) -> FinalClosestFielder {
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
            let max_reach_height = 2.5;

            match ball.trajectory {
                // Case 1 & 2: Fly, pop-up, or high liner
                TrajectoryType::Fly | TrajectoryType::PopUp | TrajectoryType::Liner => {
                    if ball_height > max_reach_height {
                        // Angle and timing are right, but it's too high even for a jump catch!
                        // ⇒ Let it through without touching, continue the loop
                        continue;
                    }
                }
                // Grounders always have height 0 and never go over the head (proceed to catch)
                TrajectoryType::Grounder => {}
            }

            // --- If we reach here, the height is within reach (catch successful)! ---
            let is_fly_catch = match ball.trajectory {
                TrajectoryType::Liner | TrajectoryType::Fly | TrajectoryType::PopUp => true, // No-bounce catch
                TrajectoryType::Grounder => false,
            };

            return FinalClosestFielder {
                fielder: fielder.clone(),
                ball_arrival_time: ball_arrival_time,
                is_fly_catch: is_fly_catch,
            };
        }
    }

    // 3. Nobody touched it and it got through to the outfield (same as before: closest outfielder handles it)
    let final_closest = find_closest_fielder(fielders, &ball);
    FinalClosestFielder {
        fielder: final_closest,
        ball_arrival_time: ball.hang_time,
        is_fly_catch: false,
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
    fn try_catch_returns_out_when_fielder_arrives_before_airborne_ball_lands() {
        let center_fielder = fielder(Position::CF, 75.0, 0.0);
        let fly_ball = ball(TrajectoryType::Fly, 80.0, 0.0, 3.0, 120.0, 35.0);

        let result = center_fielder.try_catch(&fly_ball);

        assert_eq!(result.ruling, Ruling::Out);
        assert_near(result.time_to_catch, 0.4 + (5.0 / 7.0));
        assert_near(result.final_distance, 80.0);
    }

    #[test]
    fn try_catch_returns_safe_and_bounded_distance_when_airborne_ball_falls_in() {
        let center_fielder = fielder(Position::CF, 60.0, 0.0);
        let liner = ball(TrajectoryType::Liner, 80.0, 0.0, 1.0, 100.0, 15.0);

        let result = center_fielder.try_catch(&liner);

        assert_eq!(result.ruling, Ruling::Safe);
        assert!(result.time_to_catch > liner.hang_time);
        assert!(result.final_distance > liner.distance());
    }

    #[test]
    fn try_catch_treats_grounders_as_safe_for_later_base_race() {
        let shortstop = fielder(Position::SS, 30.0, -5.0);
        let grounder = ball(TrajectoryType::Grounder, 32.0, -5.0, 0.9, 95.0, 4.0);

        let result = shortstop.try_catch(&grounder);

        assert_eq!(result.ruling, Ruling::Safe);
        assert_near(result.time_to_catch, 0.4 + (2.0 / 7.0));
        assert_near(result.final_distance, 32.0);
    }

    #[test]
    fn liner_beyond_fielder_adds_reaction_delay() {
        let center_fielder = fielder(Position::CF, 80.0, 0.0);
        let liner_beyond_fielder = ball(TrajectoryType::Liner, 90.0, 0.0, 2.0, 110.0, 15.0);

        let result = center_fielder.try_catch(&liner_beyond_fielder);

        assert_eq!(result.ruling, Ruling::Out);
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

        let result = process_defensive_chain(&fielders, &grounder);

        assert_eq!(result.fielder.position, Position::SS);
        assert!(!result.is_fly_catch);
        assert_near(result.ball_arrival_time, 3.0 * (28.0 / 90.0));
    }

    #[test]
    fn process_defensive_chain_skips_fielder_when_airborne_ball_is_over_reach() {
        let fielders = [
            fielder(Position::SS, 35.0, 0.0),
            fielder(Position::CF, 80.0, 0.0),
        ];
        let fly_ball = ball(TrajectoryType::Fly, 85.0, 0.0, 3.5, 130.0, 35.0);

        let result = process_defensive_chain(&fielders, &fly_ball);

        assert_eq!(result.fielder.position, Position::CF);
        assert!(result.is_fly_catch);
    }

    #[test]
    fn process_defensive_chain_falls_back_to_closest_fielder_when_lane_misses() {
        let fielders = [
            fielder(Position::SS, 30.0, -30.0),
            fielder(Position::CF, 78.0, 0.0),
        ];
        let grounder = ball(TrajectoryType::Grounder, 80.0, 25.0, 2.0, 90.0, 5.0);

        let result = process_defensive_chain(&fielders, &grounder);

        assert_eq!(result.fielder.position, Position::CF);
        assert!(!result.is_fly_catch);
        assert_near(result.ball_arrival_time, grounder.hang_time);
    }

    #[test]
    fn evaluate_throw_play_records_out_when_defense_beats_runner() {
        let first_baseman = fielder(Position::FB, 28.0, 40.0);
        let runner = Runner {
            speed: 7.0,
            current_base: Base::Home,
            lead_distance: 0.0,
        };
        let catch_pos = PolarPosition::new(20.0, 35.0);

        let result = evaluate_throw_play(
            0.8,
            &first_baseman,
            catch_pos,
            Base::First,
            PlayType::ForcePlay,
            &runner,
            RL::Left,
        );

        assert_eq!(result.ruling, Ruling::Out);
        assert_eq!(result.action, DefenseAction::Throw);
        assert!(result.defense_time < result.runner_time);
        assert_near(
            result.time_difference,
            (result.defense_time - result.runner_time).abs(),
        );
    }

    #[test]
    fn evaluate_throw_play_selects_self_touch_when_running_to_base_is_faster() {
        let first_baseman = Fielder::new(Position::FB, 26.0, 45.0, 5.0, 8.0, 0.4, 0.9);
        let runner = Runner {
            speed: 8.5,
            current_base: Base::Home,
            lead_distance: 0.0,
        };
        let catch_pos = PolarPosition::new(25.0, 45.0);

        let result = evaluate_throw_play(
            5.0,
            &first_baseman,
            catch_pos,
            Base::First,
            PlayType::TouchPlay,
            &runner,
            RL::Right,
        );

        assert_eq!(result.action, DefenseAction::SelfTouch);
        assert_eq!(result.ruling, Ruling::Safe);
        assert!(result.defense_time > result.runner_time);
    }
}
