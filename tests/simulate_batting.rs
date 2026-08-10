mod common;

use common::*;
use rusqlite::params;
use saberbb::domain::random_provider::*;
use saberbb::domain::resolver::batting_resolver::*;
use saberbb::domain::resolver::pitching_resolver::*;
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
    conn.execute("DELETE FROM test_batted_ball", []).unwrap();

    let pitcher = generate_pitcher();
    let batter = generate_batter();

    let mut rng = RealRng::new();

    for _ in 0..1000 {
        let pitched_ball = create_pitch(&mut rng, &pitcher).unwrap();
        let expected_ball = create_pitch(&mut rng, &pitcher).unwrap();

        let matchup = MatchupContext {
            throw_side: pitcher.throw_side,
            batting_side: batter.batting_side,
        };

        let pitch_displacement = calculate_pitch_offset(&pitched_ball, &matchup, &expected_ball);
        let timing_offset = calculate_timing_offset(&mut rng, &pitched_ball, &pitched_ball);

        let intended_location = BallLocation {
            x: pitched_ball.actual_location.x * rng.normal_factor_std_1_percent(),
            y: pitched_ball.actual_location.y * rng.normal_factor_std_1_percent(),
        };

        let swing_execution_error = calculate_swing_execution_error(
            batter.bat_contact,
            &intended_location,
            &pitched_ball.actual_location,
        );

        let contact = evaluate_swing_contact(
            &batter,
            &pitch_displacement,
            timing_offset,
            &swing_execution_error,
        );

        let batted_ball = calculate_batted_ball(&batter, pitched_ball, &contact);

        conn.execute(
            "INSERT INTO test_batted_ball (
            additional_x_m, additional_z_m, actual_bat_angle_deg,
            offset_x_m, offset_z_m, thickness_offset_m, length_offset_m, timing_offset_sec, contact_type,
            launch_speed_ms, launch_angle,  spray_angle, distance_m, hang_time_sec, trajectory) 
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                swing_execution_error.additional_x_m, swing_execution_error.additional_z_m, swing_execution_error.actual_bat_angle_deg,
                contact.offset_x_m, contact.offset_z_m, contact.thickness_offset_m, contact.length_offset_m, contact.timing_offset, contact.contact_type.as_ref(),
                batted_ball.launch_speed, batted_ball.launch_angle, batted_ball.angle(), batted_ball.distance(),  batted_ball.hang_time, batted_ball.trajectory.as_ref()
                ],
        )
        .unwrap();
    }
}
