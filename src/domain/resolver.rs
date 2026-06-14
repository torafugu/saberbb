use crate::domain::shared::ball::{Ball, TrajectoryType};
use crate::domain::shared::game::BattingResult;
use crate::domain::shared::player::{Player, Position, RL};
use kurbo::Point;
use rand::RngExt;
use rand_distr::{Distribution, Normal};

// TODO: merge into Player
#[derive(Clone)]
pub struct Fielder {
    pub position: Position,
    pub distance: f64,
    pub angle: f64,
    pub speed: f64,    // Running speed (m/s) e.g. 6.5 – 8.0 m/s
    pub reaction: f64, // Reaction time (seconds) e.g. 0.3 – 0.7 s (lower is better)
}
impl Fielder {
    fn try_catch(&self, ball: &Ball) -> bool {
        // 1. Calculate straight-line distance from position to landing point
        let required_distance =
            calculate_distance(self.distance, self.angle, ball.distance, ball.spray_angle);

        // 2. Convert to distance and angle to position
        let p1 = calculate_position(self.distance, self.angle);
        let p2 = calculate_position(ball.distance, ball.spray_angle);
        let dy = p1.y - p2.y;

        // 3. Adjust initial reaction speed based on hit type (secret ingredient)
        let mut final_reaction = self.reaction;
        if ball.trajectory == TrajectoryType::Liner && dy < 0.0 {
            // Delay reaction when moving forward on a liner (harder to judge)
            final_reaction += 0.15;
        }

        // 4. Calculate arrival time (seconds)
        let arrival_time = final_reaction + (required_distance / self.speed);

        // 5. Compare arrival time vs hang time
        arrival_time <= ball.hang_time
    }
}

pub fn calculate_distance(p1_distance: f64, p1_angle: f64, p2_distance: f64, p2_angle: f64) -> f64 {
    // Convert the difference between the two angles to radians.
    let angle_diff_rad = (p1_angle - p2_angle).to_radians();

    // Apply the law of cosines.
    let cos_val = angle_diff_rad.cos();
    let distance_squared = (p1_distance * p1_distance) + (p2_distance * p2_distance)
        - (2.0 * p1_distance * p2_distance * cos_val);

    // Guard against rare negative values caused by floating-point error.
    distance_squared.max(0.0).sqrt()
}

fn calculate_position(distance: f64, angle_deg: f64) -> Point {
    // Rust's sin/cos require radians, so convert from degrees
    let angle_rad = angle_deg.to_radians();

    // Axes are swapped, so x is sin and y is cos
    let x = distance * angle_rad.sin();
    let y = distance * angle_rad.cos();

    Point::new(x, y)
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

    // Step 3: Randomly sample within the range
    // TODO: Change to normal (Gaussian) distribution
    let final_angle = rng.random_range(min_angle..max_angle) as f64;

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

    Ball {
        launch_speed: launch_speed,
        launch_angle: launch_angle,
        spray_angle: spray_angle,
        distance: distance,
        hang_time: hang_time,
        trajectory: trajectory,
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
                    if ball.distance < 50.0 {
                        // Infield grounder: only infielders (1B, 2B, 3B, SS) are candidates
                        matches!(
                            f.position,
                            Position::FB | Position::SB | Position::TB | Position::SS
                        )
                    } else {
                        // Grounder through to the outfield: outfielders handle it
                        matches!(f.position, Position::LF | Position::CF | Position::RF)
                    }
                }
                // For fly balls and liners
                _ => {
                    if ball.distance < 45.0 {
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
            let dist_a = calculate_distance(a.distance, a.angle, ball.distance, ball.spray_angle);
            let dist_b = calculate_distance(b.distance, b.angle, ball.distance, ball.spray_angle);

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
    use crate::domain::resolver::{
        Fielder, TrajectoryType, calculate_batted_ball, find_closest_fielder, sample_spray_angle,
    };
    use crate::domain::shared::ball::Ball;
    use crate::domain::shared::player::Position;
    use crate::domain::shared::player::RL;

    #[test]
    fn test_fielders_try_catch() {
        let fb = Fielder {
            position: Position::FB,
            distance: 35.0,
            angle: 33.0,
            speed: 7.0,
            reaction: 0.5,
        };

        let sb = Fielder {
            position: Position::SB,
            distance: 40.0,
            angle: 18.0,
            speed: 7.0,
            reaction: 0.5,
        };

        let tb = Fielder {
            position: Position::TB,
            distance: 35.0,
            angle: -33.0,
            speed: 7.0,
            reaction: 0.5,
        };

        let ss = Fielder {
            position: Position::SS,
            distance: 40.0,
            angle: -18.0,
            speed: 7.0,
            reaction: 0.5,
        };

        let rf = Fielder {
            position: Position::RF,
            distance: 80.0,
            angle: 26.0,
            speed: 7.0,
            reaction: 0.5,
        };

        let cf = Fielder {
            position: Position::CF,
            distance: 90.0,
            angle: 0.0,
            speed: 7.0,
            reaction: 0.5,
        };

        let lf = Fielder {
            position: Position::LF,
            distance: 80.0,
            angle: -26.0,
            speed: 7.0,
            reaction: 0.5,
        };

        let fielders: [Fielder; 7] = [fb, sb, tb, ss, rf, cf, lf];

        let ball = Ball {
            launch_speed: 100.0, // km/h
            launch_angle: 20.0,  // Z arc degree
            spray_angle: 15.0,   // X arc degree
            distance: 90.0,      // m
            hang_time: 10.0,     // second
            trajectory: TrajectoryType::Fly,
        };

        let handler = find_closest_fielder(&fielders, &ball);

        println!("Who?:{}", handler.position);
        println!("Catch?:{}", handler.try_catch(&ball));
    }

    #[test]
    fn test_1b_try_catch() {
        let fb = Fielder {
            position: Position::FB,
            distance: 35.0,
            angle: 33.0,
            speed: 7.0, // Running speed (m/s) e.g. 6.5 – 8.0 m/s
            reaction: 0.5,
        };

        let ball = Ball {
            launch_speed: 100.0, // km/h
            launch_angle: 20.0,  // Z arc degree
            spray_angle: 34.0,   // X arc degree
            distance: 20.0,      // m
            hang_time: 3.0,      // second
            trajectory: TrajectoryType::Grounder,
        };

        println!("Catch?:{}", fb.try_catch(&ball));
    }

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
        assert!((-45.0..45.0).contains(&ball.spray_angle));

        match ball.trajectory {
            TrajectoryType::Grounder => assert!((0.0..10.0).contains(&ball.launch_angle)),
            TrajectoryType::Liner => assert!((10.0..25.0).contains(&ball.launch_angle)),
            TrajectoryType::Fly => assert!((25.0..50.0).contains(&ball.launch_angle)),
            TrajectoryType::PopUp => assert!((50.0..80.0).contains(&ball.launch_angle)),
        }
    }
}
