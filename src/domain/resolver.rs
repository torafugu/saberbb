use crate::domain::shared::ball::{Ball, PolarPosition, TrajectoryType};
use crate::domain::shared::game::{BASE_DISTANCE, BattingResult};
use crate::domain::shared::game_state::Ruling;
use crate::domain::shared::player::{Player, Position, RL};
use crate::domain::shared::stadium::Base;
use rand::RngExt;
use rand_distr::{Distribution, Normal, StandardNormal};

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

struct Runner {
    speed: f64, // Base running speed (m/s) e.g. 7.7
    current_base: Base,
    lead_distance: f64, // Current lead distance (m), valid when current_base > 0
}

impl Runner {
    // Returns the actual running distance to the next base
    fn get_running_distance(&self, batting_side: RL) -> f64 {
        match self.current_base {
            Base::Home => {
                // Batter-runner case (lead is 0, distance adjusted by batting side)
                match batting_side {
                    RL::Right => BASE_DISTANCE + 2.0, // Right batter's box is farther
                    RL::Left => BASE_DISTANCE,        // Left batter's box is shortest
                }
            }
            _ => {
                // Runner on base case (subtract lead from base distance)
                (BASE_DISTANCE - self.lead_distance).max(0.0)
            }
        }
    }
}

#[derive(Debug)]
pub struct CatchStatus {
    pub ruling: Ruling,
    pub time_to_catch: f64,
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

    pub fn try_catch(&self, ball: &Ball) -> CatchStatus {
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
        let ruling = if arrival_time <= ball.hang_time {
            Ruling::Out
        } else {
            Ruling::Safe
        };

        CatchStatus {
            ruling: ruling,
            time_to_catch: arrival_time,
        }
    }
}

struct HitBallResult {
    final_distance: f64, // Final distance where the ball stopped (or hit the fence)
    time_to_fumble: f64, // Total time until the fielder picks up the ball
}

fn apply_fence_bounce(calculated_distance: f64, angle: f64) -> (f64, f64) {
    // If it hasn't reached the fence, return coordinates as-is
    if calculated_distance <= FENCE_DISTANCE {
        return (calculated_distance, angle);
    }

    // Distance past the fence (the distance that would have gone into the stands)
    let overflow = calculated_distance - FENCE_DISTANCE;

    // Distance after bounce (back toward infield from the fence)
    let final_distance = FENCE_DISTANCE - (overflow * FENCE_BOUNCE_COEFF);

    // Angle stays the same (vertical collision with curved fence)
    let final_angle = angle;

    // Safety net: stop at fence at minimum
    (final_distance.max(0.0), final_angle)
}

// Processing when a fly/liner wasn't caught (became a hit)
fn process_outfield_hit(
    air_distance: f64, // Landing point distance
    hang_time: f64,    // Descent time
    launch_speed_kmh: f64,
    launch_angle_deg: f64,
    trajectory: &TrajectoryType,
    closest_fielder: &Fielder, // Closest fielder who was chasing
) -> HitBallResult {
    let v = launch_speed_kmh * 0.278; // m/s
    let theta = launch_angle_deg.to_radians();

    // 1. Damping coefficient at the moment of the first bounce (liner bounces sharply, fly dies)
    let k_impact = match trajectory {
        TrajectoryType::Liner => 0.60,
        TrajectoryType::Fly => 0.35,
        _ => 0.0,
    };

    // 2. Initial speed as a grounder right after the bounce
    let v_horizontal = v * theta.cos() * 0.7; // Velocity including in-flight air resistance
    let v_bounce = v_horizontal * k_impact;

    // 3. Additional rolling distance and time until stop
    let roll_distance = v_bounce * 1.8;
    let roll_time = if v_bounce > 0.0 {
        roll_distance / (v_bounce * 0.5)
    } else {
        0.0
    };

    // 4. Provisional final resting position (landing point + roll distance)
    let mut final_distance = air_distance + roll_distance;

    // The fence bounce (cushion) logic naturally applies here too!
    let mut total_roll_time = roll_time;
    if final_distance > FENCE_DISTANCE {
        let overflow = final_distance - FENCE_DISTANCE;
        final_distance = FENCE_DISTANCE - (overflow * 0.25);
        // If it hits the fence, rolling time stops there
        total_roll_time *= (FENCE_DISTANCE - air_distance) / roll_distance;
    }

    // 5. Defense: time for the fielder to chase down and pick up the rolling ball
    // The fielder was initially running toward the landing point but didn't make it.
    // Simple calculation of time to loop around toward the direction the ball rolled (final_distance)
    let ball_stop_time = hang_time + total_roll_time; // Time when the ball stops

    // Time for the fielder to reach the final resting point (or cushion treatment position)
    let base_pos = Base::Home.polar_position(); // Simplified: same straight-line calculation
    let fielder_distance_to_ball = (final_distance - closest_fielder.distance()).abs();
    let fielder_arrival_time =
        closest_fielder.reaction + (fielder_distance_to_ball / closest_fielder.running_speed);

    // Time the fielder picks up the ball (either waiting for it to stop or cutting it off mid-roll)
    let time_to_pick_up = fielder_arrival_time.max(hang_time + 0.5); // At least 0.5s after the first bounce

    HitBallResult {
        final_distance,
        time_to_fumble: time_to_pick_up, // ★This becomes the time_to_catch for the next throw play!
    }
}

fn calculate_distance(p1: &PolarPosition, p2: &PolarPosition) -> f64 {
    // Convert the difference between the two angles to radians.
    let angle_diff_rad = (p1.angle - p2.angle).to_radians();

    // Apply the law of cosines.
    let cos_val = angle_diff_rad.cos();
    let distance_squared = (p1.distance * p1.distance) + (p2.distance * p2.distance)
        - (2.0 * p1.distance * p2.distance * cos_val);

    // Guard against rare negative values caused by floating-point error.
    distance_squared.max(0.0).sqrt()
}

// batted-ball direction (sector)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldSector {
    Pull,      // Pull (right-handed batter → left field, left-handed batter → right field)
    Center,    // Center field
    Opposite, // Opposite field (right-handed batter → right field, left-handed batter → left field)
    FoulLeft, // Third-base-side foul
    FoulRight, // First-base-side foul
}

// TODO: merge into Player
// Hitter's batted-ball direction ability and tendency data
pub struct Batter {
    pub batting_side: RL,
    pub swing_speed: f64,

    // Weight of probability for each sector (set to sum to 1.0)
    // TODO: Randomize based on batter type
    pub weight_pull: f32,
    pub weight_center: f32,
    pub weight_opposite: f32,
    pub weight_foul_left: f32,
    pub weight_foul_right: f32,
}
impl Batter {
    // Returns the concrete angle range (min, max) for the selected sector
    fn get_angle_range(&self, sector: FieldSector) -> (f32, f32) {
        match self.batting_side {
            RL::Right => match sector {
                FieldSector::FoulLeft => (-90.0, -45.0),
                FieldSector::Pull => (-45.0, -15.0), // Right-handed batter's pull → left field (-)
                FieldSector::Center => (-15.0, 15.0),
                FieldSector::Opposite => (15.0, 45.0), // Right-handed batter's opposite → right field (+)
                FieldSector::FoulRight => (45.0, 90.0),
            },
            RL::Left => match sector {
                FieldSector::FoulLeft => (-90.0, -45.0),
                FieldSector::Opposite => (-45.0, -15.0), // Left-handed batter's opposite → left field (-)
                FieldSector::Center => (-15.0, 15.0),
                FieldSector::Pull => (15.0, 45.0), // Left-handed batter's pull → right field (+)
                FieldSector::FoulRight => (45.0, 90.0),
            },
        }
    }
}

fn inner_choose_sector(batter: &Batter) -> FieldSector {
    let mut rng = rand::rng();
    let total_weight = batter.weight_pull
        + batter.weight_center
        + batter.weight_opposite
        + batter.weight_foul_left
        + batter.weight_foul_right;
    let mut roll = rng.random_range(0.0..total_weight);

    if roll < batter.weight_pull {
        return FieldSector::Pull;
    }
    roll -= batter.weight_pull;

    if roll < batter.weight_center {
        return FieldSector::Center;
    }
    roll -= batter.weight_center;

    if roll < batter.weight_opposite {
        return FieldSector::Opposite;
    }
    roll -= batter.weight_opposite;

    if roll < batter.weight_foul_left {
        return FieldSector::FoulLeft;
    }
    return FieldSector::FoulRight;
}

fn sample_spray_angle(tendency: &Batter) -> f64 {
    let mut rng = rand::rng();

    // Step 1: Decide the sector
    let chosen_sector = inner_choose_sector(tendency);

    // Step 2: Get the angle range for that sector
    let (min_angle, max_angle) = tendency.get_angle_range(chosen_sector);
    let min_angle = min_angle as f64;
    let max_angle = max_angle as f64;

    // Step 3: Randomly sample within the range
    let mean = (min_angle + max_angle) * 0.5;
    let std_dev = (max_angle - min_angle) / 6.0;
    let final_angle =
        (mean + std_dev * rng.sample::<f64, _>(StandardNormal)).clamp(min_angle, max_angle);

    final_angle
}

fn calculate_height_at_distance(
    launch_speed: f64,
    launch_angle: f64,
    trajectory: &TrajectoryType,
    target_distance: f64, // Distance at which to calculate height (m)
) -> f64 {
    let v = launch_speed * 0.278; // Convert to m/s
    let theta = launch_angle.to_radians();
    let g = 9.8;

    // 1. Apply drag coefficient based on trajectory type
    let kd = match trajectory {
        TrajectoryType::Liner => 0.75,
        TrajectoryType::Fly | TrajectoryType::PopUp => 0.55,
        TrajectoryType::Grounder => return 0.0, // Grounder height is always 0
    };

    // 2. Back-calculate time (t) to reach the target distance
    let horizontal_velocity = v * theta.cos() * kd;
    if horizontal_velocity <= 0.0 {
        return 0.0;
    } // Error guard

    let t = target_distance / horizontal_velocity;

    // 3. Calculate height at that time using the parabolic formula
    let initial_vertical_velocity = v * theta.sin();
    let height = (initial_vertical_velocity * t) - (0.5 * g * t * t);

    // Clamp to 0m if negative (ball would be below ground)
    height.max(0.0)
}

pub fn calculate_batted_ball(batter: &Batter, pitch_speed: f64) -> Ball {
    let mut rng = rand::rng();

    // TODO: decide TrajectoryType mod_slg and meet type should be considered
    let trajectory = match rng.random_range(0..4) {
        0 => TrajectoryType::Liner,
        1 => TrajectoryType::Fly,
        2 => TrajectoryType::Grounder,
        _ => TrajectoryType::PopUp,
    };

    // 1. Theoretical maximum exit velocity for a squared-up ball (V_max)
    // $$V_{\text{max}} = (A \times V_{\text{swing}}) + (B \times V_{\text{pitch}})$$
    let a = 1.15; // Swing efficiency
    let b = 0.20; // Rebound efficiency
    let v_max = (a * batter.swing_speed) + (b * pitch_speed);

    // 2. Randomly select the damping factor based on TrajectoryType (contact quality)
    let contact_efficiency = match &trajectory {
        TrajectoryType::Liner => rng.random_range(0.85..1.00),
        TrajectoryType::Fly => rng.random_range(0.70..0.92),
        TrajectoryType::Grounder => rng.random_range(0.65..0.90),
        TrajectoryType::PopUp => rng.random_range(0.40..0.60),
    };

    // 3. Determine the base exit velocity
    let mut base_speed = v_max * contact_efficiency;

    // 4. Add the final variation with normally distributed noise (mean 0, standard deviation 5 km/h)
    let normal_dist = Normal::new(0.0, 5.0).unwrap();
    let noise = normal_dist.sample(&mut rng);

    base_speed += noise;

    // Cap the minimum value to prevent negative or excessively slow speeds
    let launch_speed = base_speed.max(30.0);

    let launch_angle: f64 = match &trajectory {
        TrajectoryType::Grounder => rng.random_range(0.0..10.0),
        TrajectoryType::Liner => rng.random_range(10.0..25.0),
        TrajectoryType::Fly => rng.random_range(25.0..50.0),
        TrajectoryType::PopUp => rng.random_range(50.0..80.0),
    };
    let spray_angle = sample_spray_angle(batter);

    let v = launch_speed * 0.278; // Convert to m/s
    let theta = launch_angle.to_radians();
    let g = 9.8;

    let (distance, hang_time) = match trajectory {
        TrajectoryType::Fly | TrajectoryType::PopUp => {
            let kt = 0.95; // Hang time correction
            let kd = 0.55; // Distance drag correction
            let time = (2.0 * v * theta.sin()) / g * kt;
            let dist = (v * theta.cos() * time) * kd;
            (dist, time)
        }
        TrajectoryType::Liner => {
            let kt = 1.0;
            let kd = 0.75; // Liner drives lose less speed
            let time = (2.0 * v * theta.sin()) / g * kt;
            let dist = (v * theta.cos() * time) * kd;
            (dist, time)
        }
        TrajectoryType::Grounder => {
            // Grounder-specific calculation for infield arrival time and final rolling distance
            let time_to_infield = 30.0 / (v * theta.cos() * 0.8);
            let total_dist = v * 1.5 + rand::random_range(-5.0..5.0);
            (total_dist, time_to_infield)
        }
    };

    Ball::new(
        launch_speed,
        launch_angle,
        spray_angle,
        distance,
        hang_time,
        trajectory,
    )
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

fn find_final_outfield_wrapper(fielders: &[Fielder], distance: f64, angle: f64) -> Fielder {
    let temp_ball = Ball::new(0.0, 0.0, angle, distance, 0.0, TrajectoryType::Grounder);
    find_closest_fielder(fielders, &temp_ball)
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

// Evaluate fielders on the trajectory lane from front to back (revised over-the-head version)
fn process_defensive_chain(
    fielders: &[Fielder],
    ball: &Ball,
    final_air_distance: f64, // Landing point (fly) or final stopping point (grounder)
    hang_time_to_final: f64,
) -> (Fielder, f64, bool) {
    // 1. Sort fielders in the same lane by distance (closest first)
    let mut lane_fielders: Vec<&Fielder> = fielders
        .iter()
        .filter(|f| is_ball_in_fielder_lane(f, ball.angle()))
        .filter(|f| f.distance() <= final_air_distance)
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
        let ratio = fielder_dist / final_air_distance;
        let ball_arrival_time = hang_time_to_final * ratio;

        // Time for the fielder to move laterally to intercept the ball's line
        let dx = fielder_dist * (fielder.angle() - ball.angle()).to_radians().sin();
        let required_move_distance = dx.abs();
        let fielder_arrival_time =
            fielder.reaction + (required_move_distance / fielder.running_speed);

        // [Judgment A] Can the fielder reach the spot laterally before the ball passes through?
        if fielder_arrival_time <= ball_arrival_time {
            // ★ Core: calculate the ball's height as it passes over the fielder's head!
            let ball_height = calculate_height_at_distance(
                ball.launch_speed,
                ball.launch_angle,
                &ball.trajectory,
                fielder_dist, // Calculate height at this fielder's distance
            );

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

            return (fielder.clone(), ball_arrival_time, is_fly_catch);
        }
    }

    // 3. Nobody touched it and it got through to the outfield (same as before: closest outfielder handles it)
    let final_closest = find_final_outfield_wrapper(fielders, final_air_distance, ball.angle());
    (final_closest, hang_time_to_final, false)
}

pub fn simulate_batting(batter: &Player) -> BattingResult {
    let rng: f64 = rand::random();
    let result: BattingResult;
    // TODO: Adjust by mod_slg!
    let xbh_average: f64 = batter.slg() - batter.hit_average();
    let double_average: f64 = batter.hit_average() + xbh_average * 0.5;
    let triple_average: f64 = batter.hit_average() + xbh_average * 0.6;
    let home_run_average: f64 = batter.hit_average() + xbh_average;

    match rng {
        n if batter.hit_average() > n => result = BattingResult::Single,
        n if double_average > n => result = BattingResult::Double,
        n if triple_average > n => result = BattingResult::Triple,
        n if home_run_average > n => result = BattingResult::HomeRun,
        _ => result = BattingResult::Out,
    }
    result
}

#[cfg(test)]
mod tests {
    use crate::domain::resolver::Batter;
    use crate::domain::resolver::{TrajectoryType, calculate_batted_ball, sample_spray_angle};
    use crate::domain::shared::player::RL;

    #[test]
    fn test_spray_angle() {
        let right_average_hitter = Batter {
            batting_side: RL::Right,
            swing_speed: 150.0,
            weight_pull: 0.35,
            weight_center: 0.35,
            weight_opposite: 0.15,
            weight_foul_left: 0.08,
            weight_foul_right: 0.07,
        };

        let angle = sample_spray_angle(&right_average_hitter);
        println!("angle:{}", angle);
    }

    #[test]
    fn test_calculate_batted_ball() {
        let right_average_hitter = Batter {
            batting_side: RL::Right,
            swing_speed: 150.0,
            weight_pull: 0.35,
            weight_center: 0.35,
            weight_opposite: 0.15,
            weight_foul_left: 0.08,
            weight_foul_right: 0.07,
        };

        let ball = calculate_batted_ball(&right_average_hitter, 150.0);
        assert!((-45.0..45.0).contains(&ball.angle()));

        match ball.trajectory {
            TrajectoryType::Grounder => assert!((0.0..10.0).contains(&ball.launch_angle)),
            TrajectoryType::Liner => assert!((10.0..25.0).contains(&ball.launch_angle)),
            TrajectoryType::Fly => assert!((25.0..50.0).contains(&ball.launch_angle)),
            TrajectoryType::PopUp => assert!((50.0..80.0).contains(&ball.launch_angle)),
        }
    }
}
