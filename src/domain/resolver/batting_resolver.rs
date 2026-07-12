use crate::domain::shared::ball::{BattedBall, TrajectoryType};
use crate::domain::shared::game::BattingResult;
use crate::domain::shared::player::{BatterInfo, Player};
use crate::domain::util::GRAVIY;
use rand::RngExt;
use rand_distr::{Distribution, Normal, StandardNormal};
use serde::{Deserialize, Serialize};
use strum_macros::{AsRefStr, EnumIter, EnumString};

// batted-ball direction (sector)
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, EnumIter, AsRefStr,
)]
#[strum(ascii_case_insensitive)]
pub enum FieldSector {
    Pull,      // NOTE: Pull (right-handed batter → left field, left-handed batter → right field)
    Center,    // NOTE: Center field
    Opposite, // NOTE: Opposite field (right-handed batter → right field, left-handed batter → left field)
    FoulLeft, // NOTE: Third-base-side foul
    FoulRight, // NOTE: First-base-side foul
}

fn inner_choose_sector(batter: &BatterInfo) -> FieldSector {
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

// TODO: rng should be passed as parameter
fn sample_spray_angle(tendency: &BatterInfo) -> f64 {
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

// TODO: rng should be passed as parameter
pub fn calculate_batted_ball(batter: &BatterInfo, pitch_speed: f64) -> BattedBall {
    let mut rng = rand::rng();

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

    let (distance, hang_time) = match trajectory {
        TrajectoryType::Fly | TrajectoryType::PopUp => {
            let kt = 0.95; // Hang time correction
            let kd = 0.55; // Distance drag correction
            let time = (2.0 * v * theta.sin()) / GRAVIY * kt;
            let dist = (v * theta.cos() * time) * kd;
            (dist, time)
        }
        TrajectoryType::Liner => {
            let kt = 1.0;
            let kd = 0.75; // Liner drives lose less speed
            let time = (2.0 * v * theta.sin()) / GRAVIY * kt;
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

    BattedBall::new(
        launch_speed,
        launch_angle,
        spray_angle,
        distance,
        hang_time,
        trajectory,
    )
}

// pub fn simulate_batting(batter: &Player) -> BattingResult {
//     let rng: f64 = rand::random();
//     let result: BattingResult;
//     // TODO: Adjust by mod_slg!
//     let xbh_average: f64 = batter.slg() - batter.hit_average();
//     let double_average: f64 = batter.hit_average() + xbh_average * 0.5;
//     let triple_average: f64 = batter.hit_average() + xbh_average * 0.6;
//     let home_run_average: f64 = batter.hit_average() + xbh_average;

//     match rng {
//         n if batter.hit_average() > n => result = BattingResult::Single,
//         n if double_average > n => result = BattingResult::Double,
//         n if triple_average > n => result = BattingResult::Triple,
//         n if home_run_average > n => result = BattingResult::HomeRun,
//         _ => result = BattingResult::Out,
//     }
//     result
// }

#[cfg(test)]
mod tests {
    use crate::domain::resolver::batting_resolver::{
        BatterInfo, FieldSector, calculate_batted_ball, inner_choose_sector, sample_spray_angle,
    };
    use crate::domain::shared::ball::TrajectoryType;
    use crate::domain::shared::player::RL;

    fn batter_with_weights(
        batting_side: RL,
        weight_pull: f64,
        weight_center: f64,
        weight_opposite: f64,
        weight_foul_left: f64,
        weight_foul_right: f64,
    ) -> BatterInfo {
        BatterInfo {
            batting_side,
            swing_speed: 150.0,
            weight_pull,
            weight_center,
            weight_opposite,
            weight_foul_left,
            weight_foul_right,
        }
    }

    fn assert_between(value: f64, min: f64, max: f64) {
        assert!(
            value >= min && value <= max,
            "{} was outside [{}, {}]",
            value,
            min,
            max
        );
    }

    #[test]
    fn batter_get_angle_range_maps_pull_and_opposite_by_batting_side() {
        let right_hitter = batter_with_weights(RL::Right, 1.0, 0.0, 0.0, 0.0, 0.0);
        let left_hitter = batter_with_weights(RL::Left, 1.0, 0.0, 0.0, 0.0, 0.0);

        assert_eq!(
            right_hitter.get_angle_range(FieldSector::Pull),
            (-45.0, -15.0)
        );
        assert_eq!(
            right_hitter.get_angle_range(FieldSector::Opposite),
            (15.0, 45.0)
        );
        assert_eq!(left_hitter.get_angle_range(FieldSector::Pull), (15.0, 45.0));
        assert_eq!(
            left_hitter.get_angle_range(FieldSector::Opposite),
            (-45.0, -15.0)
        );
    }

    #[test]
    fn inner_choose_sector_returns_the_only_weighted_sector() {
        let cases = [
            (
                batter_with_weights(RL::Right, 1.0, 0.0, 0.0, 0.0, 0.0),
                FieldSector::Pull,
            ),
            (
                batter_with_weights(RL::Right, 0.0, 1.0, 0.0, 0.0, 0.0),
                FieldSector::Center,
            ),
            (
                batter_with_weights(RL::Right, 0.0, 0.0, 1.0, 0.0, 0.0),
                FieldSector::Opposite,
            ),
            (
                batter_with_weights(RL::Right, 0.0, 0.0, 0.0, 1.0, 0.0),
                FieldSector::FoulLeft,
            ),
            (
                batter_with_weights(RL::Right, 0.0, 0.0, 0.0, 0.0, 1.0),
                FieldSector::FoulRight,
            ),
        ];

        for (batter, expected_sector) in cases {
            assert_eq!(inner_choose_sector(&batter), expected_sector);
        }
    }

    #[test]
    fn sample_spray_angle_stays_inside_forced_sector_range() {
        let cases = [
            (
                batter_with_weights(RL::Right, 1.0, 0.0, 0.0, 0.0, 0.0),
                -45.0,
                -15.0,
            ),
            (
                batter_with_weights(RL::Right, 0.0, 1.0, 0.0, 0.0, 0.0),
                -15.0,
                15.0,
            ),
            (
                batter_with_weights(RL::Right, 0.0, 0.0, 1.0, 0.0, 0.0),
                15.0,
                45.0,
            ),
            (
                batter_with_weights(RL::Left, 1.0, 0.0, 0.0, 0.0, 0.0),
                15.0,
                45.0,
            ),
            (
                batter_with_weights(RL::Left, 0.0, 0.0, 1.0, 0.0, 0.0),
                -45.0,
                -15.0,
            ),
            (
                batter_with_weights(RL::Left, 0.0, 0.0, 0.0, 1.0, 0.0),
                -90.0,
                -45.0,
            ),
            (
                batter_with_weights(RL::Left, 0.0, 0.0, 0.0, 0.0, 1.0),
                45.0,
                90.0,
            ),
        ];

        for (batter, min_angle, max_angle) in cases {
            for _ in 0..20 {
                assert_between(sample_spray_angle(&batter), min_angle, max_angle);
            }
        }
    }

    #[test]
    fn calculate_batted_ball_sets_physical_values_and_trajectory_specific_launch_angle() {
        let right_pull_hitter = batter_with_weights(RL::Right, 1.0, 0.0, 0.0, 0.0, 0.0);

        for _ in 0..50 {
            let ball = calculate_batted_ball(&right_pull_hitter, 150.0);

            assert!(ball.launch_speed_kmh >= 30.0);
            assert!(ball.distance().is_finite());
            assert!(ball.hang_time.is_finite());
            assert_between(ball.angle(), -45.0, -15.0);

            match ball.trajectory {
                TrajectoryType::Grounder => assert_between(ball.launch_angle, 0.0, 10.0),
                TrajectoryType::Liner => assert_between(ball.launch_angle, 10.0, 25.0),
                TrajectoryType::Fly => assert_between(ball.launch_angle, 25.0, 50.0),
                TrajectoryType::PopUp => assert_between(ball.launch_angle, 50.0, 80.0),
            }
        }
    }
}
