mod common;

use common::*;
use rusqlite::params;
use saberbb::domain::random_provider::*;
use saberbb::domain::resolver::batting_resolver::*;
use saberbb::domain::shared::ball::*;
use saberbb::domain::shared::game::*;
use saberbb::domain::shared::player::*;
use saberbb::domain::strategy::pitch_call::TargetZone;
use saberbb::domain::util::*;
use saberbb::repositories::db::*;

#[test]
fn test_stand_in() {
    let stadium = generate_stadium();
    let ball = BattedBall::new(170.0, 35.0, 30.0, 130.0, 5.0, TrajectoryType::Fly);
    println!("x:{}, y:{}", ball.x(), ball.y());

    if stadium.is_stand_in(&ball) {
        if ball.is_foul() {
            println!("{}", BattingResult::Foul);
        } else {
            println!("{}", BattingResult::HomeRun);
        }
    } else {
        println!("In ground !"); // Hit, Direct hit on the fence
    }
}

#[test]
fn test_ball_height() {
    let ball = BattedBall::new(160.0, 35.0, 30.0, 300.0, 5.0, TrajectoryType::Fly);
    let heigt = ball.calculate_height_at_distance(100.0);
    println!("height:{}", heigt);
}

#[test]
fn test_batted_ball() {
    let conn = SqlDb::new().unwrap().get_conn().unwrap();

    let right_average_hitter = BatterInfo {
        batting_side: RL::Right,
        swing_speed: 125.0,
        base_launch_angle: 28.0,
        consistency_sigma: 0.03,
        weight_pull: 0.35,
        weight_center: 0.35,
        weight_opposite: 0.15,
        weight_foul_pull: 0.08,
        weight_foul_opposite: 0.07,
    };

    let mut rng = RealRng::new();

    for _ in 0..1000 {
        let ball = calculate_batted_ball(
            &mut rng,
            &right_average_hitter,
            PitchedBall {
                pitch_type: PitchType::FourSeamFastball,
                speed_kmh: 150.0,
                spin_rate: 2300.0,
                spin_angle: 30.0,
                spin_efficiency: 0.95,
                release_point: Vector3D {
                    x: 1.6,
                    y: 16.74,
                    z: 1.7,
                },
                flight_time: 0.42,
                aim_zone: TargetZone::Center,
                aim_location: BallLocation { x: 0.0, y: 0.0 },
                actual_location: BallLocation { x: 0.0, y: 0.0 },
            },
            &SwingContactResult {
                vertical_offset: 0.0,
                horizontal_offset: -0.1,
                timing_offset: 0.0,
            },
        );
        conn.execute(
            "INSERT INTO test_batted_ball (launch_speed_kmh, launch_angle,  spray_angle, distance, hang_time, trajectory) 
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![ball.launch_speed_kmh, ball.launch_angle, ball.angle(), ball.distance(),  ball.hang_time, ball.trajectory.as_ref()],
        )
        .unwrap();
    }
}
