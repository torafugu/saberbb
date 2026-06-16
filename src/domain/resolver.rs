use crate::domain::shared::ball::{Ball, PolarPosition, TrajectoryType};
use crate::domain::shared::game::{BASE_DISTANCE, BattingResult};
use crate::domain::shared::player::{Player, Position, RL};
use crate::domain::shared::stadium::Base;
use rand::RngExt;
use rand_distr::{Distribution, Normal, StandardNormal};

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
    is_out: bool,
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
    let is_out = total_defense_time <= total_runner_time;
    let time_difference = (total_defense_time - total_runner_time).abs();

    PlayResult {
        is_out,
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
    pub fn try_catch(&self, ball: &Ball) -> f64 {
        // 1. Calculate straight-line distance from position to landing point
        let required_distance = calculate_distance(&self.polar_position, &ball.polar_position);
        let dy = self.polar_position.y - ball.polar_position.y;

        // 3. Adjust initial reaction speed based on hit type (secret ingredient)
        let mut final_reaction = self.reaction;
        if ball.trajectory == TrajectoryType::Liner && dy < 0.0 {
            // Delay reaction when moving forward on a liner (harder to judge)
            final_reaction += 0.15;
        }

        // 4. Calculate arrival time (seconds)
        final_reaction + (required_distance / self.running_speed)

        // 5. Compare arrival time vs hang time
        // arrival_time <= ball.hang_time
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
    // let spray_angle = rng.random_range(-45.0..45.0);

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
                    if ball.polar_position.distance < 50.0 {
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
                    if ball.polar_position.distance < 45.0 {
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
        assert!((-45.0..45.0).contains(&ball.polar_position.angle));

        match ball.trajectory {
            TrajectoryType::Grounder => assert!((0.0..10.0).contains(&ball.launch_angle)),
            TrajectoryType::Liner => assert!((10.0..25.0).contains(&ball.launch_angle)),
            TrajectoryType::Fly => assert!((25.0..50.0).contains(&ball.launch_angle)),
            TrajectoryType::PopUp => assert!((50.0..80.0).contains(&ball.launch_angle)),
        }
    }
}
