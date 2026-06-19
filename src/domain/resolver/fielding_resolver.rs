use crate::domain::shared::ball::{Ball, TrajectoryType};
use crate::domain::shared::game::BASE_DISTANCE;
use crate::domain::shared::game_state::Ruling;
use crate::domain::shared::player::{Position, RL};
use crate::domain::shared::stadium::Base;
use crate::domain::util::{PolarPosition, calculate_distance};

// TODO: fence distance should be retrieved the stadium
const FENCE_DISTANCE: f64 = 100.0; // Stadium fence distance (assumed 100m)
const FENCE_BOUNCE_COEFF: f64 = 0.25; // Fence bounce coefficient (grounder cushion is quite damped)

// 塁上のランナー状態を表すビットマスク（0〜7の値をとる）
// 例: 一・三塁にランナーがいる場合 = 1 + 4 = 5 (101)
const RUNNER_NONE: u8 = 0; // ランナーなし (000)
const RUNNER_1ST: u8 = 1; // 一塁ランナー (001)
const RUNNER_2ND: u8 = 2; // 二塁ランナー (010)
const RUNNER_3RD: u8 = 4; // 三塁ランナー (100)
const RUNNER_FULL: u8 = 7;
const RUNNER_1ST_AND_2ND: u8 = 3;

// 塁上の各ランナーの走力 (ランナーがいない場合は None)
struct RunnersOnBase {
    batter_speed: f64, // バッターは常に存在するので f32
    runner_1st_speed: Option<f64>,
    runner_2nd_speed: Option<f64>,
    runner_3rd_speed: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayType {
    ForcePlay,
    TouchPlay,
}

#[derive(Debug)]
struct PlayContext<'a> {
    bases_occupied: u8,
    fielder: &'a Fielder,
    catch_position: PolarPosition,
    is_hit: bool,       // 内野ゴロならfalse、外野ヒットや内野安打ならtrue
    time_to_catch: f64, // 捕球（またはヒット処理）にかかった時間
}

#[derive(Debug)]
struct AutoTarget {
    base: Base,
    play_type: PlayType,
}

// 塁上状態と、捕球した野手のポジションから、狙うべき最適なベースを自動判定する
fn judge_optimal_target_general(ctx: &PlayContext) -> AutoTarget {
    // A. 内野ゴロ（アウトが取れる状況）の場合 → 以前のロジック
    if !ctx.is_hit {
        if ctx.bases_occupied == RUNNER_FULL {
            return AutoTarget {
                base: Base::Home,
                play_type: PlayType::ForcePlay,
            };
        }
        if (ctx.bases_occupied & RUNNER_3RD) == RUNNER_3RD && ctx.catch_position.distance <= 25.0 {
            return AutoTarget {
                base: Base::Home,
                play_type: PlayType::TouchPlay,
            };
        }
        if (ctx.bases_occupied & RUNNER_1ST_AND_2ND) == RUNNER_1ST_AND_2ND
            && matches!(ctx.fielder.position, Position::TB | Position::SS)
        {
            return AutoTarget {
                base: Base::Third,
                play_type: PlayType::ForcePlay,
            };
        }
        if (ctx.bases_occupied & RUNNER_1ST) == RUNNER_1ST {
            if ctx.catch_position.distance <= 25.0
                && matches!(
                    ctx.fielder.position,
                    Position::P | Position::C | Position::FB | Position::TB
                )
            {
                return AutoTarget {
                    base: Base::First,
                    play_type: PlayType::ForcePlay,
                };
            }
            if matches!(
                ctx.fielder.position,
                Position::SB | Position::SS | Position::P
            ) {
                return AutoTarget {
                    base: Base::Second,
                    play_type: PlayType::ForcePlay,
                };
            }
        }
        return AutoTarget {
            base: Base::First,
            play_type: PlayType::ForcePlay,
        };
    }

    // B. 外野ヒット（長打・単打）の場合 → 今回の一般化拡張
    // 1. 三塁（または二塁）にランナーがいて、バックホームが間に合いそうな場合
    // ※「捕球にかかった時間」が短く、かつ外野手が比較的浅い（80m以内）ならホームで刺しにいく
    if (ctx.bases_occupied & (RUNNER_2ND | RUNNER_3RD)) != 0 {
        if ctx.catch_position.distance <= 80.0 && ctx.time_to_catch <= 3.5 {
            return AutoTarget {
                base: Base::Home,
                play_type: PlayType::TouchPlay,
            };
        }
    }

    // 2. 一塁にランナーがいて、ライト前ヒットなどで三塁進塁を阻止したい場合
    if (ctx.bases_occupied & RUNNER_1ST) == RUNNER_1ST {
        // レフト前なら三塁は諦めることが多いが、ライト・センター前なら三塁で刺せる可能性がある
        if matches!(ctx.fielder.position, Position::CF | Position::RF)
            && ctx.catch_position.distance <= 75.0
        {
            return AutoTarget {
                base: Base::Third,
                play_type: PlayType::TouchPlay,
            };
        }
    }

    // 3. 完全な長打（長打コースを外野手がフェンス際 95m~ で処理している場合）
    // バッターが二塁（あるいは三塁）に突っ込んでくるのを阻止する
    if ctx.catch_position.distance >= 90.0 {
        return AutoTarget {
            base: Base::Second,
            play_type: PlayType::TouchPlay,
        };
    }

    // 4. その他（普通の単打で、ランナーの進塁も無理がない場合）
    // 内野へ返球してプレイを落ち着かせる（便宜上一番近い内野ベースにする）
    AutoTarget {
        base: Base::Second,
        play_type: PlayType::TouchPlay,
    }
}

#[derive(Debug)]
struct PlayResult {
    ruling: Ruling,
    defense_time: f64,
    runner_time: f64,
    time_difference: f64, // For determining if it's a close play
}

fn evaluate_defense_play(
    ctx: &PlayContext,
    runners: &RunnersOnBase,
    lead_distance: f64,
    batting_side: RL, // Batter's side; only used for batter-runner distance adjustment
) -> PlayResult {
    // 1. 最適なターゲットベースとプレイ種類を自動判定
    let target = judge_optimal_target_general(ctx);
    let base_pos = target.base.polar_position();

    // 2. 守備総時間の計算
    let distance_to_base = calculate_distance(&ctx.catch_position, &base_pos);
    let mut defense_play_time = if target.play_type == PlayType::ForcePlay {
        let time_via_throw = ctx.fielder.prep_time + (distance_to_base / ctx.fielder.throw_speed);
        let time_via_run = distance_to_base / ctx.fielder.running_speed;
        time_via_run.min(time_via_throw)
    } else {
        ctx.fielder.prep_time + (distance_to_base / ctx.fielder.throw_speed) + 0.3
    };
    let total_defense_time = ctx.time_to_catch + defense_play_time;

    // 3. ターゲットに応じたランナーの「距離」と「走力」の動的抽出（Option対応版）
    // パターンマッチで対象ベースの Option<f64> を紐解く
    let (running_distance, target_runner_speed): (f64, Option<f64>) = match target.base {
        Base::First => {
            let dist = match batting_side {
                RL::Right => BASE_DISTANCE + 2.0,
                RL::Left => BASE_DISTANCE,
            };
            (dist, Some(runners.batter_speed)) // バッターは常に存在する
        }
        Base::Second => (
            (BASE_DISTANCE - lead_distance).max(0.0),
            runners.runner_1st_speed,
        ),
        Base::Third => (
            (BASE_DISTANCE - lead_distance).max(0.0),
            runners.runner_2nd_speed,
        ),
        Base::Home => (
            (BASE_DISTANCE - lead_distance).max(0.0),
            runners.runner_3rd_speed,
        ),
    };

    // 4. ランナータイムの計算と安全弁（ガードロジック）
    let total_runner_time = match target_runner_speed {
        Some(speed) => {
            // ランナーが正常に存在する場合、通常通りタイムレースの時間を計算
            (running_distance / speed) + 0.5
        }
        None => {
            // 【重要】もしランナーがいないベースに投げてしまった場合（ロジックエラーや大野選）
            // ランナータイムを「0秒」にして、守備側を強制的にセーフ（失敗）にする安全弁
            0.0
        }
    };

    // 5. 勝敗判定
    // ランナータイムが0.0（None）の場合は、必ず total_defense_time > 0.0 になるため、
    // is_out = false（セーフ / 野選扱い）になり、ゲームがクラッシュするのを防ぎます。
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
    }
}

#[derive(Debug)]
pub struct FieldingResult {
    pub ruling: Ruling,
    pub time_to_catch: f64,
    pub final_distance: f64,
    pub is_fly_catch: bool,
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

        let mut is_fly_catch = match ball.trajectory {
            TrajectoryType::Liner | TrajectoryType::Fly | TrajectoryType::PopUp => true, // No-bounce catch
            TrajectoryType::Grounder => false,
        };

        // 5. Compare arrival time vs hang time
        let (ruling, time_to_catch, final_distance) = if ball.trajectory == TrajectoryType::Grounder
        {
            // Ruling delegates to time race
            (Ruling::Safe, arrival_time, ball.distance())
        } else if arrival_time <= ball.hang_time {
            (Ruling::Out, arrival_time, ball.distance())
        } else {
            is_fly_catch = false;
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
            is_fly_catch: is_fly_catch,
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

pub fn find_closest_fielder<'a>(fielders: &'a [Fielder], ball: &'a Ball) -> &'a Fielder {
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
pub struct FinalClosestFielder<'a> {
    pub fielder: &'a Fielder,
    pub ball: &'a Ball,
}

// Evaluate fielders on the trajectory lane from front to back (revised over-the-head version)
pub fn process_defensive_chain<'a>(
    fielders: &'a [Fielder],
    ball: &'a mut Ball,
) -> FinalClosestFielder<'a> {
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
        let ctx = PlayContext {
            bases_occupied: RUNNER_FULL,
            fielder: &pitcher,
            catch_position: PolarPosition::new(18.0, 0.0),
            is_hit: false,
            time_to_catch: 0.8,
        };

        let target = judge_optimal_target_general(&ctx);

        assert_target(target, Base::Home, PlayType::ForcePlay);
    }

    #[test]
    fn judge_optimal_target_general_chooses_third_force_for_left_side_grounder() {
        let third_baseman = fielder(Position::TB, 32.0, -35.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_1ST_AND_2ND,
            fielder: &third_baseman,
            catch_position: PolarPosition::new(30.0, -35.0),
            is_hit: false,
            time_to_catch: 1.0,
        };

        let target = judge_optimal_target_general(&ctx);

        assert_target(target, Base::Third, PlayType::ForcePlay);
    }

    #[test]
    fn judge_optimal_target_general_throws_home_on_shallow_hit_with_lead_runner() {
        let center_fielder = fielder(Position::CF, 75.0, 0.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_3RD,
            fielder: &center_fielder,
            catch_position: PolarPosition::new(70.0, 0.0),
            is_hit: true,
            time_to_catch: 3.0,
        };

        let target = judge_optimal_target_general(&ctx);

        assert_target(target, Base::Home, PlayType::TouchPlay);
    }

    #[test]
    fn judge_optimal_target_general_chooses_third_on_shallow_center_hit_with_runner_on_first() {
        let center_fielder = fielder(Position::CF, 75.0, 0.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_1ST,
            fielder: &center_fielder,
            catch_position: PolarPosition::new(72.0, 5.0),
            is_hit: true,
            time_to_catch: 3.8,
        };

        let target = judge_optimal_target_general(&ctx);

        assert_target(target, Base::Third, PlayType::TouchPlay);
    }

    #[test]
    fn judge_optimal_target_general_chooses_second_for_deep_extra_base_hit() {
        let left_fielder = fielder(Position::LF, 90.0, -25.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_NONE,
            fielder: &left_fielder,
            catch_position: PolarPosition::new(95.0, -20.0),
            is_hit: true,
            time_to_catch: 5.0,
        };

        let target = judge_optimal_target_general(&ctx);

        assert_target(target, Base::Second, PlayType::TouchPlay);
    }

    #[test]
    fn evaluate_defense_play_records_out_when_force_play_beats_batter_runner() {
        let first_baseman = fielder(Position::FB, 28.0, 40.0);
        let catch_position = PolarPosition::new(20.0, 35.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_NONE,
            fielder: &first_baseman,
            catch_position: catch_position.clone(),
            is_hit: false,
            time_to_catch: 0.8,
        };
        let runners = runners_on_base(None, None, None);

        let result = evaluate_defense_play(&ctx, &runners, 0.0, RL::Left);
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
    fn evaluate_defense_play_records_safe_when_tag_play_loses_home_race() {
        let left_fielder = fielder(Position::LF, 78.0, -20.0);
        let catch_position = PolarPosition::new(78.0, -20.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_3RD,
            fielder: &left_fielder,
            catch_position: catch_position.clone(),
            is_hit: true,
            time_to_catch: 3.2,
        };
        let runners = runners_on_base(None, None, Some(8.0));

        let result = evaluate_defense_play(&ctx, &runners, 4.0, RL::Right);
        let throw_distance = calculate_distance(&catch_position, &Base::Home.polar_position());
        let expected_defense_time =
            3.2 + left_fielder.prep_time + (throw_distance / left_fielder.throw_speed) + 0.3;
        let expected_runner_time = (BASE_DISTANCE - 4.0) / 8.0 + 0.5;

        assert_eq!(result.ruling, Ruling::Safe);
        assert!(result.defense_time > result.runner_time);
        assert_near(result.defense_time, expected_defense_time);
        assert_near(result.runner_time, expected_runner_time);
    }

    #[test]
    fn evaluate_defense_play_returns_safe_when_selected_target_has_no_runner() {
        let center_fielder = fielder(Position::CF, 90.0, 0.0);
        let ctx = PlayContext {
            bases_occupied: RUNNER_NONE,
            fielder: &center_fielder,
            catch_position: PolarPosition::new(95.0, 0.0),
            is_hit: true,
            time_to_catch: 5.0,
        };
        let runners = runners_on_base(None, None, None);

        let result = evaluate_defense_play(&ctx, &runners, 0.0, RL::Left);

        assert_eq!(result.ruling, Ruling::Safe);
        assert_near(result.runner_time, 0.0);
        assert!(result.defense_time > 0.0);
    }
}
