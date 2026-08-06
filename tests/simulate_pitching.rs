mod common;

use common::*;
use rusqlite::params;
use saberbb::domain::random_provider::*;
use saberbb::domain::resolver::pitching_resolver::*;
use saberbb::repositories::db::*;

#[test]
fn test_pitched_ball() {
    let conn = SqlDb::new().unwrap().get_conn().unwrap();
    conn.execute("DELETE FROM test_pitched_ball", []).unwrap();

    let mut rng = RealRng::new();

    let pitcher = generate_pitcher();
    let batter = generate_batter();

    for _ in 0..1000 {
        let ball = create_pitch(&mut rng, &pitcher).unwrap();
        let expected_ball = create_pitch(&mut rng, &pitcher).unwrap();

        let ball_movement = calculate_ball_movement(&ball);

        let matchup = MatchupContext {
            throw_side: pitcher.throw_side,
            batting_side: batter.batting_side,
        };

        let pitch_displacement = calculate_pitch_offset(&ball, &matchup, &expected_ball);
        let timing_offset = calculate_timing_offset(&mut rng, &ball, &expected_ball);

        conn.execute(
            "INSERT INTO test_pitched_ball (pitch_type, speed_kmh,  spin_rate, spin_angle, spin_efficiency, 
            release_point_x, release_point_y, release_point_z, flight_time, aim_zone, 
            aim_x, aim_y, actual_x, actual_y, pitch_result, movement_x, movement_z, disp_x, disp_y, timing) 
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
            params![
                ball.pitch_type.as_ref(),
                ball.speed_kmh,
                ball.spin_rate,
                ball.spin_angle,
                ball.spin_efficiency,
                ball.release_point.x,
                ball.release_point.y,
                ball.release_point.z,
                ball.flight_time,
                ball.aim_zone.as_ref(),
                ball.aim_location.x,
                ball.aim_location.y,
                ball.actual_location.x,
                ball.actual_location.y,
                ball.actual_location.call().as_ref(),
                ball_movement.x_m,
                ball_movement.z_m,
                pitch_displacement.horizontal_offset_m,
                pitch_displacement.vertical_offset_m,
                timing_offset,
            ],
        )
        .unwrap();
    }
}
