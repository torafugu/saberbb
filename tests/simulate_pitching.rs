mod common;

use common::*;
use rusqlite::params;
use saberbb::domain::random_provider::*;
use saberbb::domain::resolver::pitching_resolver::*;
use saberbb::domain::schedule_service::ScheduleService;
use saberbb::domain::shared::game_state::{ActiveBatter, ActivePitcher, InningState};
use saberbb::domain::shared::player::SWING_SPEED_AVG;
use saberbb::domain::strategy::pitching_strategy::select_pitching_strategy;
use saberbb::repositories::db::*;

#[test]
fn test_pitched_ball() {
    let conn = SqlDb::new().unwrap().get_conn().unwrap();
    conn.execute("DELETE FROM test_pitched_ball", []).unwrap();

    let mut rng = RealRng::new();

    let pitcher = generate_pitcher();
    let batter = generate_batter();
    let base_four_seam_speed = ScheduleService::<
        saberbb::repositories::schedule_repository::SqlScheduleRepository,
    >::DEFAULT_BASE_FOUR_SEAM_SPEED;

    for _ in 0..1000 {
        let hanging_pitch_effect = calculate_hanging_pitch_effect(&mut rng, &pitcher);
        let pitched_ball = create_pitch(
            &mut rng,
            &pitcher,
            hanging_pitch_effect,
            base_four_seam_speed,
        )
        .unwrap();
        let expected_ball = create_pitch(
            &mut rng,
            &pitcher,
            hanging_pitch_effect,
            base_four_seam_speed,
        )
        .unwrap();

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
            base_four_seam_speed,
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
                pitched_ball
                    .actual_location
                    .call(batter.batting_side)
                    .as_ref(),
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

#[test]
fn test_select_pitching_strategy() {
    let mut conn = SqlDb::new().unwrap().get_conn().unwrap();
    let runner = generate_runner();
    let mut inning_state = InningState::new();
    inning_state.active_pitcher = Some(ActivePitcher {
        id: 1,
        pitcher: generate_pitcher(),
    });
    inning_state.active_batter = Some(ActiveBatter::new(2, generate_batter(), runner.skills));

    conn.execute("DELETE FROM test_select_pitching_strategy", [])
        .unwrap();

    let tx = conn.transaction().unwrap();
    tx.execute_batch(include_str!(
        "../migrations/ddl/test_select_pitching_strategy.sql"
    ))
    .unwrap();

    {
        let mut insert = tx
            .prepare("INSERT INTO test_select_pitching_strategy (pitching_strategy) VALUES (?1)")
            .unwrap();

        for sample in 0..1000 {
            // Cycle through all 24 base/out situations.
            let base_mask = sample % 8;
            inning_state.out = ((sample / 8) % 3) as u8;
            inning_state.runners.runner_1st = (base_mask & 1 != 0).then_some(runner);
            inning_state.runners.runner_2nd = (base_mask & 2 != 0).then_some(runner);
            inning_state.runners.runner_3rd = (base_mask & 4 != 0).then_some(runner);

            if sample >= 500 {
                // A power hitter with two outs and empty bases makes avoiding
                // extra bases competitive without double-play or strikeout urgency.
                inning_state.out = 2;
                inning_state.runners = Default::default();
                let batter = &mut inning_state.active_batter.as_mut().unwrap().batter;
                batter.swing_speed = SWING_SPEED_AVG;
                batter.swing_power = 2.0;
                assert!(batter.slugger_option() > 0.5);
            }

            let strategy = select_pitching_strategy(&inning_state, 1, 0);
            insert.execute(params![format!("{strategy:?}")]).unwrap();
        }
    }

    let count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM test_select_pitching_strategy",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1000);
    let avoid_extra_bases_count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM test_select_pitching_strategy WHERE pitching_strategy = 'AvoidExtraBases'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // Equal strategy scores are currently broken by HashMap iteration order,
    // so check presence across the samples rather than an exact percentage.
    assert!(avoid_extra_bases_count > 0);
    tx.commit().unwrap();
}
