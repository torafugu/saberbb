mod common;

use common::*;
use rusqlite::{named_params, params};
use saberbb::domain::random_provider::*;
use saberbb::domain::resolver::batting_resolver::CountStatus;
use saberbb::domain::resolver::pitching_resolver::*;
use saberbb::domain::schedule_service::ScheduleService;
use saberbb::domain::shared::game_state::{ActiveBatter, ActivePitcher, InningState};
use saberbb::domain::shared::player::SWING_SPEED_AVG;
use saberbb::domain::strategy::pitching_strategy::{
    PitchEvaluationContext, PitchingStrategy, catcher_preferences, evaluate_pitch_call,
    pitcher_preferences, reconcile_pitch_call_proposals, select_pitching_strategy,
};
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
    const SAMPLES: usize = 1000;
    const COUNTS: [(u8, u8, CountStatus); 12] = [
        (0, 0, CountStatus::C00),
        (1, 0, CountStatus::C10),
        (2, 0, CountStatus::C20),
        (3, 0, CountStatus::C30),
        (0, 1, CountStatus::C01),
        (1, 1, CountStatus::C11),
        (2, 1, CountStatus::C21),
        (3, 1, CountStatus::C31),
        (0, 2, CountStatus::C02),
        (1, 2, CountStatus::C12),
        (2, 2, CountStatus::C22),
        (3, 2, CountStatus::C32),
    ];
    let mut conn = SqlDb::new().unwrap().get_conn().unwrap();
    let runner = generate_runner();
    let pitcher = generate_pitcher();
    let catcher = generate_catcher();
    let mut inning_state = InningState::new();
    inning_state.active_pitcher = Some(ActivePitcher {
        id: 1,
        pitcher: pitcher.clone(),
    });
    inning_state.active_batter = Some(ActiveBatter::new(2, generate_batter(), runner.skills));

    let tx = conn.transaction().unwrap();
    tx.execute_batch(include_str!(
        "../migrations/ddl/test_select_pitching_strategy.sql"
    ))
    .unwrap();

    {
        let mut insert = tx
            .prepare(
                "INSERT INTO test_select_pitching_strategy (
                sample, balls, strikes, outs, runner_mask, pitching_strategy,
                pitch_type, target_zone, margin, aim_x, aim_y,
                score, pitcher_score, catcher_score, decision_reason,
                pitcher_candidates, catcher_candidates,
                previous_pitch_type, previous_target_zone, previous_margin
            ) VALUES (
                :sample, :balls, :strikes, :outs, :runner_mask, :strategy,
                :pitch_type, :target_zone, :margin, :aim_x, :aim_y,
                :score, :pitcher_score, :catcher_score, :reason,
                :pitcher_candidates, :catcher_candidates,
                :previous_pitch_type, :previous_target_zone, :previous_margin
            )",
            )
            .unwrap();

        let mut last_call = None;
        for sample in 0..SAMPLES {
            // Cycle through all 24 base/out situations.
            let base_mask = sample % 8;
            inning_state.out = ((sample / 8) % 3) as u8;
            inning_state.runners.runner_1st = (base_mask & 1 != 0).then_some(runner);
            inning_state.runners.runner_2nd = (base_mask & 2 != 0).then_some(runner);
            inning_state.runners.runner_3rd = (base_mask & 4 != 0).then_some(runner);
            // Exercise each count against all 24 base/out situations.
            let (balls, strikes, count) = COUNTS[(sample / 24) % COUNTS.len()];
            inning_state.ball = balls;
            inning_state.strike = strikes;
            // These are synthetic situations, not a simulated plate appearance.
            // Reuse the last selected call to exercise sequence preferences;
            // an initial count has no previous call.
            let previous_call = if balls == 0 && strikes == 0 {
                None
            } else {
                last_call
            };

            if sample >= 500 {
                // Isolate a power hitter with poor contact and plate discipline.
                // Without pitcher bonuses, AvoidExtraBases wins the deterministic
                // tie with PitchAround in this low-walk-cost situation.
                inning_state.out = 2;
                inning_state.runners = Default::default();
                inning_state.active_pitcher = None;
                let batter = &mut inning_state.active_batter.as_mut().unwrap().batter;
                batter.swing_speed = SWING_SPEED_AVG;
                batter.swing_power = 2.0;
                batter.batting_eye = -3.0;
                batter.bat_control = -3.0;
                assert!(batter.slugger_option() > 0.5);
                assert!(batter.score() <= 0.5);
            }

            let strategy = select_pitching_strategy(&inning_state, 1, 0);
            if sample >= 500 {
                assert_eq!(strategy, PitchingStrategy::AvoidExtraBases);
            }
            let batter = &inning_state.active_batter.as_ref().unwrap().batter;
            // The controlled strategy regression omits pitcher bonuses, but
            // call generation still needs the actual pitcher's repertoire.
            let pitcher_preferences =
                pitcher_preferences(pitcher.pitcher_character, batter, count, previous_call);
            let catcher_preferences =
                catcher_preferences(catcher.calling_style, batter, count, previous_call);
            let context = PitchEvaluationContext::new(&pitcher, batter, strategy, previous_call);
            let pitcher_proposals = context.pitcher_proposals(&pitcher_preferences, 3, 2);
            let catcher_proposals = context.catcher_proposals(&catcher_preferences, 4);
            assert!(!pitcher_proposals.is_empty());
            assert!(!catcher_proposals.is_empty());
            let decision = reconcile_pitch_call_proposals(
                &pitcher_proposals,
                &catcher_proposals,
                &pitcher_preferences,
                &catcher_preferences,
                &context,
                0.5,
            )
            .expect("an available repertoire must produce a final PitchCall");
            let pitch_call = decision.pitch_call;
            assert!(
                pitcher
                    .pitch_skills
                    .iter()
                    .any(|p| p.pitch_type == pitch_call.pitch_type)
            );
            assert!(
                pitcher_proposals
                    .iter()
                    .chain(&catcher_proposals)
                    .any(|p| p.pitch_call == pitch_call)
            );
            let pitcher_score = evaluate_pitch_call(pitch_call, &pitcher_preferences, &context)
                .unwrap()
                .score;
            let catcher_score = evaluate_pitch_call(pitch_call, &catcher_preferences, &context)
                .unwrap()
                .score;
            assert_eq!(decision.pitcher_score, Some(pitcher_score));
            assert_eq!(decision.catcher_score, Some(catcher_score));
            assert!((decision.score - (pitcher_score + catcher_score) / 2.0).abs() < 1e-9);
            // Ensure negotiation selected the best joint score within the union.
            for proposal in pitcher_proposals.iter().chain(&catcher_proposals) {
                let p = evaluate_pitch_call(proposal.pitch_call, &pitcher_preferences, &context)
                    .unwrap();
                let c = evaluate_pitch_call(proposal.pitch_call, &catcher_preferences, &context)
                    .unwrap();
                assert!(decision.score + 1e-9 >= (p.score + c.score) / 2.0);
            }
            let aim = pitch_call.aim_location();
            assert!(aim.x.is_finite() && aim.y.is_finite());
            let runner_mask = usize::from(inning_state.runners.runner_1st.is_some())
                | (usize::from(inning_state.runners.runner_2nd.is_some()) << 1)
                | (usize::from(inning_state.runners.runner_3rd.is_some()) << 2);
            insert
                .execute(named_params! {
                    ":sample": sample as i64, ":balls": balls, ":strikes": strikes,
                    ":outs": inning_state.out, ":runner_mask": runner_mask as u8,
                    ":strategy": format!("{strategy:?}"),
                    ":pitch_type": pitch_call.pitch_type.as_ref(),
                    ":target_zone": pitch_call.target_zone.as_ref(),
                    ":margin": format!("{:?}", pitch_call.margin),
                    ":aim_x": aim.x, ":aim_y": aim.y,
                    ":score": decision.score,
                    ":pitcher_score": pitcher_score, ":catcher_score": catcher_score,
                    ":reason": format!("{:?}", decision.reason),
                    ":pitcher_candidates": pitcher_proposals.len() as i64,
                    ":catcher_candidates": catcher_proposals.len() as i64,
                    ":previous_pitch_type": previous_call.map(|p| format!("{:?}", p.pitch_type)),
                    ":previous_target_zone": previous_call.map(|p| format!("{:?}", p.target_zone)),
                    ":previous_margin": previous_call.map(|p| format!("{:?}", p.margin)),
                })
                .unwrap();
            last_call = Some(pitch_call);
        }
    }

    let count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM test_select_pitching_strategy",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, SAMPLES as i64);
    let avoid_extra_bases_count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM test_select_pitching_strategy WHERE pitching_strategy = 'AvoidExtraBases'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // The controlled matchup accounts for the final 500 samples.
    assert!(avoid_extra_bases_count >= 500);
    let distinct_counts: i64 = tx.query_row(
        "SELECT COUNT(*) FROM (SELECT DISTINCT balls, strikes FROM test_select_pitching_strategy)",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(distinct_counts, COUNTS.len() as i64);
    tx.commit().unwrap();
    println!(
        "Recorded {SAMPLES} Strategy -> preferences -> proposals -> PitchCall decisions in test_select_pitching_strategy"
    );
}
