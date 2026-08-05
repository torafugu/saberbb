mod common;

use common::*;
use rusqlite::params;
use saberbb::domain::random_provider::*;
use saberbb::domain::resolver::pitching_resolver::*;
use saberbb::domain::shared::player::*;
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
            target_location TEXT NOT NULL,
            aim_x REAL NOT NULL,
            aim_y REAL NOT NULL,
            norm_x REAL NOT NULL,
            norm_y REAL NOT NULL
        )",
    )
    .unwrap();

    let mut rng = RealRng::new();

    let pitcher = generate_pitcher();
    let base_spin_angle = pitcher.base_spin_angle();

    for _ in 0..1000 {
        // let ball = create_pitch(&mut rng, &pitcher).unwrap();

        let pitch_call = pitcher.sample_pitch_calllling(&mut rng).unwrap();
        let pitch_skill = pitcher.select_pitch_skill(pitch_call.pitch_type);

        let final_spin_angle = if pitcher.throw_side == RL::Left {
            (base_spin_angle - pitch_skill.spin_angle + 360.0) % 360.0
        } else {
            (base_spin_angle + pitch_skill.spin_angle + 360.0) % 360.0
        };

        let speed = pitcher.velocity * pitch_skill.velocity * rng.normal_factor_std_1_percent();

        let speed_factor = speed / BASE_FOUR_SEAM_SPEED;

        let raw_spin_rate =
            pitch_skill.spin_rate * rng.normal_factor_std_1_percent() * speed_factor;
        let release_point = pitcher.calculate_release_point();
        let flight_time = calculate_flight_time(speed, release_point.y);

        let aim_location = pitch_call.aim_location();
        let ball_location =
            sample_ball_location(&mut rng, pitch_call.target_location.zone(), aim_location);

        conn.execute(
            "INSERT INTO test_pitched_ball (pitch_type, speed_kmh,  spin_rate, spin_angle, spin_efficiency, 
            release_point_x, release_point_y, release_point_z, flight_time, target_location, aim_x, aim_y, norm_x, norm_y) 
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                pitch_call.pitch_type.as_ref(),
                speed,
                raw_spin_rate,
                final_spin_angle,
                pitch_skill.spin_efficiency,
                release_point.x,
                release_point.y,
                release_point.z,
                flight_time,
                pitch_call.target_location.as_ref(),
                aim_location.x,
                aim_location.y,
                ball_location.x,
                ball_location.y,
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
        // let late_break = calculate_late_break_displacement(&ball, &expected_ball);
        let late_break = calculate_late_break_displacement(&ball, &ball);

        let matchup = MatchupContext {
            throw_side: pitcher.throw_side,
            batting_side: batter.batting_side,
        };
        let crossfire_multiplier = matchup.crossfire_perceived_multiplier();
        let release_x_factor = 1.0 + (ball.release_point.x.abs() * 0.15);
        let enhanced_late_break_x =
            late_break.horizontal_m * crossfire_multiplier * release_x_factor;

        let final_x = base_disp.horizontal_m + enhanced_late_break_x;
        let final_y = base_disp.vertical_m + late_break.vertical_m;

        // let timing = calculate_timing_offset(&ball, &expected_ball);
        let timing = calculate_timing_offset(&mut rng, &ball, &ball);

        conn.execute(
            "INSERT INTO test_pitched_offset (pitch_type, speed_kmh, base_disp_x, base_disp_y, late_break_x, late_break_y, 
            enhanced_late_break_x, final_x, final_y, timing) 
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                ball.pitch_type.as_ref(),
                ball.speed_kmh,
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
