mod common;

use common::*;
use saberbb::domain::random_provider::*;
use saberbb::domain::resolver::batting_resolver::*;
use saberbb::domain::resolver::fielding_resolver::*;
use saberbb::domain::resolver::pitching_resolver::*;
use saberbb::domain::shared::ball::*;
use saberbb::domain::shared::game::*;
use saberbb::domain::shared::game_state::*;
use saberbb::domain::shared::player::*;
use saberbb::domain::shared::stadium::*;
use saberbb::domain::strategy::batting_strategy::*;
use saberbb::domain::util::PolarPosition;

#[test]
fn test_through_half_inning() -> Result<(), GameError> {
    let stadium = generate_stadium();
    let batter = generate_batter();
    let fielders = generate_default_fielders();
    let pitcher = generate_pitcher();
    let catcher = generate_catcher();
    let batter_runner = generate_runner();

    let mut scores = 0;
    let mut inning_state = InningState::new();
    let mut rng = RealRng::new();

    while let InningProgress::Ongoing = inning_state.inning_progress() {
        println!("\n--- New count ---");
        inning_state.runners.batter_runner = Some(batter_runner);

        let pitched_ball = create_pitch(&mut rng, &pitcher)?;
        let expected_ball = create_pitch(&mut rng, &pitcher)?;

        let absolute_location = calculate_ball_movement(&pitched_ball);

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

        if swing_execution == SwingExecution::Take {
            println!("Take");
        } else {
            let displacement =
                adapt_to_pitch(&pitch_displacement, batter.bat_control, &batting_factor);

            let swing_error =
                calculate_swing_execution_error(&mut rng, &batter, &pitched_ball.actual_location);

            let swing_contact = evaluate_swing_contact(&batter, &displacement, &swing_error);

            if swing_contact.contact_type == SwingContactType::SwungAndMiss {
                println!("SwungAndMiss");
            } else {
                let ball = calculate_batted_ball(&batter, pitched_ball, &swing_contact, &stadium)?;

                println!("{:#?}", ball);

                match ball.outbound_result {
                    OutboundResult::Foul => {
                        println!("{}", BattingResult::Foul);
                        println!("Outs:{}, Scores:{}", inning_state.out, scores);
                        continue;
                    }
                    OutboundResult::HomeRun => {
                        scores += inning_state.runners.after_homerun();

                        println!("{}, score:+{}", BattingResult::HomeRun, scores);
                        println!("Outs:{}, Scores:{}", inning_state.out, scores);
                        continue;
                    }
                    OutboundResult::GroundRuleDouble => {
                        // TODO: Add new case to running resolver
                    }
                    OutboundResult::InField => {}
                }

                let field_play_result = process_fielding(&mut rng, &fielders, &ball)?;
                let fielder = field_play_result.result().fielder;

                println!("{:#?}", fielder);

                let fielded_ball = field_play_result.result().ball();

                println!("{:#?}", fielded_ball);

                if fielded_ball.is_fly_catch {
                    inning_state.add_out();

                    println!(
                        "Fly is caught. Outs:{}, Scores:{}",
                        inning_state.out, scores
                    );

                    if fielded_ball.fielded_by.is_infielder() || !inning_state.allows_tagup() {
                        println!("No tag-up.");
                        continue;
                    }
                }

                // TODO: stolen base tunrned into hit-and-run case
                let mut steal_attempt_rng = RealRng::new();
                if inning_state.can_steal_base(&mut steal_attempt_rng) {
                    let mut steal_defense_rng = RealRng::new();
                    let steal_defense_play_result = evaluate_base_stealing(
                        Base::Second,
                        &pitcher,
                        &catcher,
                        &mut steal_defense_rng,
                    );

                    println!("{:#?}", steal_defense_play_result);

                    let steal_runner_advance_result = inning_state
                        .runners
                        .after_base_stealing(steal_defense_play_result)?;

                    println!("{:#?}", steal_runner_advance_result);

                    if steal_runner_advance_result.ruling == Ruling::Out {
                        inning_state.add_out();
                        if inning_state.inning_progress() == InningProgress::Over {
                            break;
                        }
                    };
                }

                let ctx = PlayContext {
                    runners: &inning_state.runners,
                    fielders: &fielders,
                    try_catch_fielder: fielder,
                    fielded_ball: &fielded_ball,
                };

                let mut defense_rng = RealRng::new();
                let defense_play_result = evaluate_defense_play(&ctx, &mut defense_rng)?;

                println!("{:#?}", defense_play_result);

                let runner_advance_result = if ctx.fielded_ball.fielded_by.is_outfielder() {
                    if inning_state.allows_tagup() {
                        inning_state.runners.after_tagup(&defense_play_result)?
                    } else {
                        inning_state
                            .runners
                            .after_outfield_hit(&defense_play_result, batter.batting_side)?
                    }
                } else {
                    inning_state
                        .runners
                        .after_infield_grounder(&defense_play_result, batter.batting_side)?
                };

                println!("{:#?}", runner_advance_result);

                if ctx.fielded_ball.fielded_by.is_infielder() && inning_state.can_double_play() {
                    let mut double_play_rng = RealRng::new();
                    if let Some(double_play_defense_play_result) =
                        evaluate_double_play(&ctx, &defense_play_result, &mut double_play_rng)?
                    {
                        println!("{:#?}", double_play_defense_play_result);

                        let double_play_runner_advance_result =
                            inning_state.runners.after_double_play(
                                &double_play_defense_play_result,
                                &runner_advance_result,
                                batter.batting_side,
                            )?;

                        println!("{:#?}", double_play_runner_advance_result);

                        inning_state.runners.commit_unsaved_runners(
                            double_play_runner_advance_result.unsaved_runners,
                        );

                        if double_play_runner_advance_result.ruling == Ruling::Out {
                            inning_state.add_out();
                            if inning_state.inning_progress() == InningProgress::Ongoing {
                                break;
                            }
                        };
                    }
                } else {
                    inning_state
                        .runners
                        .commit_unsaved_runners(runner_advance_result.unsaved_runners);
                }

                if runner_advance_result.ruling == Ruling::Out {
                    inning_state.add_out();
                };
            }
        };
    }

    Ok(())
}

#[test]
fn test_inning_double_play_deterministically() -> Result<(), GameError> {
    let fielders = generate_default_fielders();

    let batter_runner = ActiveRunner {
        id: 0,
        skills: RunningSkills {
            speed: 7.0,
            lead_distance: 0.0,
            start_reaction: 0.1,
        },
    };

    let runner_on_first = ActiveRunner {
        id: 1,
        skills: RunningSkills {
            speed: 7.0,
            lead_distance: 0.0,
            start_reaction: 0.1,
        },
    };

    let mut inning_state = InningState::new();
    inning_state.runners.batter_runner = Some(batter_runner);
    inning_state.runners.runner_1st = Some(runner_on_first);

    let ball = BattedBall {
        launch_speed: 95.0,
        launch_angle: 4.0,
        spin_rate: 0.0,
        spin_angle: 0.0,
        final_position: PolarPosition::new(35.0, -25.0),
        max_height: 0.0,
        total_time: 1.0,
        first_bounce_position: Some(PolarPosition::new(0.0, -25.0)),
        first_bounce_time: Some(0.0),
        fence_impact_position: None,
        fence_impact_time: None,
        outbound_result: OutboundResult::InField,
    };

    let fielder = fielders.iter().find(|f| f.is(Position::SS)).unwrap();
    let fielded_ball = FieldedBall {
        ball,
        fielded_by: Position::SS,
        catch_position: ball.final_position,
        time_to_field: 1.0,
        is_fly_catch: false,
    };

    let ctx = PlayContext {
        runners: &inning_state.runners,
        fielders: &fielders,
        try_catch_fielder: fielder,
        fielded_ball: &fielded_ball,
    };

    let mut first_play_rng = FixedRng::new(0.1);
    let first_play = evaluate_defense_play(&ctx, &mut first_play_rng)?;
    let first_advance = inning_state
        .runners
        .after_infield_grounder(&first_play, RL::Left)?;

    assert_eq!(first_advance.ruling, Ruling::Out);

    let mut second_play_rng = FixedRng::new(0.1);
    let second_play = evaluate_double_play(&ctx, &first_play, &mut second_play_rng)?.unwrap();

    inning_state.add_out();

    let second_advance =
        inning_state
            .runners
            .after_double_play(&second_play, &first_advance, RL::Left)?;

    assert_eq!(second_advance.ruling, Ruling::Out);
    inning_state.add_out();

    assert_eq!(inning_state.out, 2);

    Ok(())
}

#[test]
fn test_inning_base_steal_deterministically() -> Result<(), GameError> {
    let pitcher = PitcherInfo {
        height: 1.85,
        extension: 1.8,
        throw_side: RL::Right,
        arm_slot: ArmSlot::ThreeQuarter,
        pitcher_style: PitcherStyle::BalancedPitcher,
        velocity: 0.0,
        spin_rate: 0.0,
        control: 0.0,
        stamina: 0.0,
        injury_proneness: 0.0,
        clutch: 0.0,
        hpp: 0.0,
        platoon_splitting: 0.0,
        delivery_motion_time: 2.0,
        consistency: 0.03,
        pitch_skills: vec![],
        fielder_info: FielderInfo {
            fielder_type: FielderType::Pitcher,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
            catching: 0.8,
            reach_height: 2.5,
            reach_range: 0.0,
        },
    };
    let catcher = CatcherInfo {
        fielder_info: FielderInfo {
            fielder_type: FielderType::Catcher,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
            catching: 0.8,
            reach_height: 2.5,
            reach_range: 0.0,
        },
    };
    let runner_on_first = ActiveRunner {
        id: 0,
        skills: RunningSkills {
            speed: 9.5,
            lead_distance: 3.0,
            start_reaction: 0.1,
        },
    };

    let mut inning_state = InningState::new();
    inning_state.runners.runner_1st = Some(runner_on_first);

    let mut steal_attempt_rng = FixedRng::new(0.1);
    assert!(inning_state.can_steal_base(&mut steal_attempt_rng));

    let mut steal_defense_rng = FixedRng::new(0.1);
    let steal_defense_play_result =
        evaluate_base_stealing(Base::Second, &pitcher, &catcher, &mut steal_defense_rng);
    let steal_runner_advance_result = inning_state
        .runners
        .after_base_stealing(steal_defense_play_result)?;

    assert_eq!(steal_runner_advance_result.ruling, Ruling::Safe);
    assert!(inning_state.runners.runner_1st.is_none());
    assert!(inning_state.runners.runner_2nd.is_some());
    assert_eq!(inning_state.out, 0);

    Ok(())
}

#[test]
fn test_draw_stadium() {
    let stadium = generate_stadium();

    stadium.draw_fence();
}

#[test]
fn test_base_running() {
    let arraival_time = BASE_DISTANCE / 7.7;
    println!("{}", arraival_time);
}
