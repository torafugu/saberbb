mod common;

use common::*;
use rusqlite::params;
use saberbb::domain::random_provider::*;
use saberbb::domain::resolver::pitching_resolver::*;
use saberbb::repositories::db::*;

#[test]
fn test_pitched_ball() {
    let conn = SqlDb::new().unwrap().get_conn().unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS test_pitched_ball (
            pitch_type TEXT NOT NULL,
            speed_kmh REAL NOT NULL,
            spin_rate REAL NOT NULL,
            spin_angle REAL NOT NULL,
            spin_efficiency REAL NOT NULL,
            release_point_x REAL NOT NULL,
            release_point_y REAL NOT NULL,
            release_point_z REAL NOT NULL,
            flight_time REAL NOT NULL,
            norm_x REAL NOT NULL,
            norm_y REAL NOT NULL
        )",
    )
    .unwrap();

    let mut rng = RealRng::new();

    let pitcher = generate_pitcher();

    for _ in 0..1000 {
        let ball = create_pitch(&mut rng, &pitcher).unwrap();

        conn.execute(
            "INSERT INTO test_pitched_ball (pitch_type, speed_kmh,  spin_rate, spin_angle, spin_efficiency, 
            release_point_x, release_point_y, release_point_z, flight_time, norm_x, norm_y) 
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
                ball.location.x,
                ball.location.y,
            ],
        )
        .unwrap();
    }
}

#[test]
fn test_pitch_offset() {
    let conn = SqlDb::new().unwrap().get_conn().unwrap();
    let mut rng = RealRng::new();

    let pitcher = generate_pitcher();
    let batter = generate_batter();

    for _ in 0..1000 {
        let ball = create_pitch(&mut rng, &pitcher).unwrap();
        let expected_ball = create_pitch(&mut rng, &pitcher).unwrap();

        let base_disp = calculate_pitch_displacement(&ball);
        let late_break = calculate_late_break_displacement(&ball, &expected_ball);

        let matchup = MatchupContext {
            throw_side: pitcher.throw_side,
            batting_side: batter.batting_side,
        };
        let crossfire_multiplier = matchup.crossfire_perceived_multiplier();
        let release_x_factor = 1.0 + (ball.release_point.x.abs() * 0.15);
        let enhanced_late_break_x =
            late_break.horizontal_m * crossfire_multiplier * release_x_factor;

        let final_x = (base_disp.horizontal_m + enhanced_late_break_x).clamp(-1.0, 1.0);
        let final_y = (base_disp.vertical_m + late_break.vertical_m).clamp(-1.0, 1.0);

        let timing = calculate_timing_offset(&ball, &expected_ball);

        conn.execute(
            "INSERT INTO test_pitched_offset (base_disp_x, base_disp_y, late_break_x, late_break_y, 
            enhanced_late_break_x, final_x, final_y, timing) 
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                base_disp.horizontal_m,
                base_disp.vertical_m,
                late_break.horizontal_m,
                late_break.vertical_m,
                enhanced_late_break_x,
                final_x,
                final_y,
                timing
            ],
        )
        .unwrap();
    }
}
