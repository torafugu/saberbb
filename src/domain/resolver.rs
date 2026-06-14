use crate::domain::shared::ball::{Ball, TrajectoryType};
use crate::domain::shared::game::BattingResult;
use crate::domain::shared::player::{Player, RL};
use rand::RngExt;
use rand_distr::{Distribution, Normal};

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
    pub swing_speed: f32,

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

pub fn sample_spray_angle(tendency: &Batter) -> f32 {
    let mut rng = rand::rng();

    // Step 1: Decide the sector
    let chosen_sector = inner_choose_sector(tendency);

    // Step 2: Get the angle range for that sector
    let (min_angle, max_angle) = tendency.get_angle_range(chosen_sector);

    // Step 3: Randomly sample within the rangeA\
    // TODO: Change to normal (Gaussian) distribution
    let final_angle = rng.random_range(min_angle..max_angle);

    final_angle
}

pub fn calculate_batted_ball(batter: &Batter, pitch_speed: f32) -> Ball {
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

    let launch_angle: f32 = match &trajectory {
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
    use crate::domain::shared::game::FielderPoint;
    use crate::domain::shared::player::Position;
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

    // #[test]
    // fn test_ball_play() {
    //     let fb_point = FielderPoint {
    //         position: Position::FB,
    //         angle: 33.0,
    //         distance: 21.0,
    //     };

    //     let sb_point = FielderPoint {
    //         position: Position::SB,
    //         angle: 18.0,
    //         distance: 41.0,
    //     };

    //     let tb_point = FielderPoint {
    //         position: Position::TB,
    //         angle: -33.0,
    //         distance: 21.0,
    //     };

    //     let ss_point = FielderPoint {
    //         position: Position::SS,
    //         angle: -18.0,
    //         distance: 41.0,
    //     };

    //     let lf_point = FielderPoint {
    //         position: Position::LF,
    //         angle: -25.0,
    //         distance: 90.0,
    //     };

    //     let cf_point = FielderPoint {
    //         position: Position::CF,
    //         angle: 0.0,
    //         distance: 100.0,
    //     };

    //     let rf_point = FielderPoint {
    //         position: Position::RF,
    //         angle: 25.0,
    //         distance: 90.0,
    //     };

    //     let right_average_hitter = Batter {
    //         batting_side: RL::Right,
    //         swing_speed: 150.0,
    //         weight_pull: 0.35,
    //         weight_center: 0.35,
    //         weight_opposite: 0.15,
    //         weight_foul_left: 0.08,
    //         weight_foul_right: 0.07,
    //     };

    //     let ball = calculate_batted_ball(&right_average_hitter, 150.0);
    //     assert!(ball.launch_speed >= 30.0);
    //     assert!(ball.distance > 0.0);
    //     assert!(ball.hang_time > 0.0);
    // }
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
