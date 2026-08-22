mod common;

use common::*;
use rusqlite::params;
use saberbb::domain::random_provider::*;
use saberbb::domain::resolver::batting_resolver::*;
use saberbb::domain::resolver::pitching_resolver::*;
use saberbb::domain::shared::ball::*;
use saberbb::domain::shared::game::*;
use saberbb::domain::shared::player::*;
use saberbb::domain::strategy::batting_strategy::*;
use saberbb::repositories::db::*;

#[test]
fn test_batted_ball() {
    let conn = SqlDb::new().unwrap().get_conn().unwrap();
    conn.execute("DELETE FROM test_batted_ball", []).unwrap();

    let stadium = generate_stadium();
    let pitcher = generate_pitcher();
    let batter = generate_batter();

    let mut rng = RealRng::new();

    for _ in 0..1000 {
        let hanging_pitch_effect = calculate_hanging_pitch_effect(&mut rng, &pitcher);
        let pitched_ball = create_pitch(&mut rng, &pitcher, hanging_pitch_effect).unwrap();
        let expected_ball = create_pitch(&mut rng, &pitcher, hanging_pitch_effect).unwrap();

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

        let count_status = CountStatus::C01;

        let batting_factor = calculate_batting_factor(
            &pitcher,
            &batter,
            pitched_ball.pitch_type,
            expected_ball.pitch_type,
            &pitched_ball.actual_location,
            &expected_ball.actual_location,
        );

        let swing_factor = calculate_swing_factor(
            batter.sample_plate_approach(&mut rng).unwrap(),
            count_status,
            pitched_ball.pitch_type,
            &batting_factor,
        );

        let swing_execution = select_swing_execution(&mut rng, swing_factor);

        let (adapted_displacement, swing_execution_error, swing_contact) = if swing_execution
            == SwingExecution::Take
        {
            (
                PitchDisplacement::default(),
                SwingExecutionError::default(),
                SwingContactResult::default(),
            )
        } else {
            let displacement =
                adapt_to_pitch(&pitch_displacement, batter.bat_control, &batting_factor);

            let swing_error =
                calculate_swing_execution_error(&mut rng, &batter, &pitched_ball.actual_location);

            let contact = evaluate_swing_contact(&batter, &displacement, &swing_error);

            (displacement, swing_error, contact)
        };

        let batted_ball = if (swing_contact.contact_type == SwingContactType::Take
            || swing_contact.contact_type == SwingContactType::SwungAndMiss)
        {
            BattedBall::default()
        } else {
            calculate_batted_ball(&batter, pitched_ball, &swing_contact, &stadium).unwrap()
        };

        conn.execute(
            "INSERT INTO test_batted_ball (
            horizontal_offset_m, vertical_offset_m, timing_offset_sec,
            swing_factor, swing_execution, adapted_x_m, adapted_z_m, adapted_timing,
            additional_x_m, additional_z_m, ideal_bat_angle_deg, actual_bat_angle_deg, ideal_attack_angle_deg, actual_attack_angle_deg,
            timing_impact_x_m, offset_x_m, offset_z_m, thickness_offset_m, length_offset_m, contact_type, modified_attack_angle_deg,
            launch_speed_ms, launch_angle,  spray_angle, distance_m, hang_time_sec, trajectory) 
            VALUES (
            ?1, ?2, ?3, 
            ?4, ?5, ?6, ?7, ?8, 
            ?9, ?10, ?11, ?12, ?13, ?14, 
            ?15, ?16, ?17, ?18, ?19, ?20, ?21, 
            ?22, ?23, ?24, ?25, ?26, ?27)",
            params![
                pitch_displacement.horizontal_offset_m, pitch_displacement.vertical_offset_m, pitch_displacement.timing_offset_sec,
                swing_factor, swing_execution.as_ref(), adapted_displacement.horizontal_offset_m, adapted_displacement.vertical_offset_m, adapted_displacement.timing_offset_sec,
                swing_execution_error.additional_x_m, swing_execution_error.additional_z_m, swing_execution_error.ideal_bat_angle_deg, swing_execution_error.actual_bat_angle_deg, swing_execution_error.ideal_attack_angle_deg, swing_execution_error.actual_attack_angle_deg,
                swing_contact.timing_impact_x_m, swing_contact.offset_x_m, swing_contact.offset_z_m, swing_contact.thickness_offset_m, swing_contact.length_offset_m, swing_contact.contact_type.as_ref(), swing_contact.attack_angle_deg,
                batted_ball.launch_speed, batted_ball.launch_angle, batted_ball.angle(), batted_ball.distance(),  batted_ball.total_time, batted_ball.trajectory().as_ref()
                ],
        )
        .unwrap();
    }
}
