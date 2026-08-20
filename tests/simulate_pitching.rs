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
        let pitched_ball = create_pitch(&mut rng, &pitcher).unwrap();
        let expected_ball = create_pitch(&mut rng, &pitcher).unwrap();

        let ball_movement = calculate_ball_movement(&pitched_ball);

        let matchup = MatchupContext {
            throw_side: pitcher.throw_side,
            batting_side: batter.batting_side,
        };

        let location_bias = calculate_location_bias(pitched_ball.actual_location);

        let pitch_displacement = calculate_pitch_offset(
            &mut rng,
            &pitched_ball,
            &expected_ball,
            &matchup,
            &location_bias,
            batter.batting_eye,
        );

        conn.execute(
            "INSERT INTO test_pitched_ball (
                pitch_type, expected_pitch_type, speed_ms,  spin_rate, spin_angle, spin_efficiency, 
                release_point_x, release_point_y, release_point_z, flight_time, aim_zone, 
                aim_x, aim_y, actual_x, actual_y, pitch_result, movement_x, movement_z,
                location_bias_x, location_bias_y, location_bias_timing, crossfire_multiplier, release_x_factor,
                disp_x, disp_y, timing) 
            VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9, ?10, ?11,
                ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                ?19, ?20, ?21, ?22, ?23,
                ?24, ?25, ?26
            )",
            params![
                pitched_ball.pitch_type.as_ref(),
                expected_ball.pitch_type.as_ref(),
                pitched_ball.speed,
                pitched_ball.spin_rate,
                pitched_ball.spin_angle,
                pitched_ball.spin_efficiency,
                pitched_ball.release_point.x,
                pitched_ball.release_point.y,
                pitched_ball.release_point.z,
                pitched_ball.flight_time,
                pitched_ball.aim_zone.as_ref(),
                pitched_ball.aim_location.x,
                pitched_ball.aim_location.y,
                pitched_ball.actual_location.x,
                pitched_ball.actual_location.y,
                pitched_ball.actual_location.call().as_ref(),
                ball_movement.x_m,
                ball_movement.z_m,
                location_bias.spatial_bias_x,
                location_bias.spatial_bias_y,
                location_bias.timing_bias_sec,
                pitch_displacement.crossfire_multiplier,
                pitch_displacement.release_x_factor,
                pitch_displacement.horizontal_offset_m,
                pitch_displacement.vertical_offset_m,
                pitch_displacement.timing_offset_sec,
            ],
        )
        .unwrap();
    }
}
