use rand::RngExt;
use rand_distr::StandardNormal;
use rusqlite::params;
use saberbb::domain::random_provider::{FixedRng, RealRng};
use saberbb::domain::resolver::batting_resolver::*;
use saberbb::domain::resolver::fielding_resolver::*;
use saberbb::domain::shared::ball::*;
use saberbb::domain::shared::game::*;
use saberbb::domain::shared::game_state::*;
use saberbb::domain::shared::player::*;
use saberbb::domain::shared::stadium::*;
use saberbb::domain::util::PolarPosition;
use saberbb::repositories::db::*;
use std::collections::HashMap;

fn generate_stadium() -> Stadium {
    Stadium::new("AAA".to_string(), 98.0, 120.0, 2.0)
}

// TODO: Retrieve from PlayerFactory
fn generate_default_fielders() -> [ActiveFielder; 9] {
    let fielders: HashMap<Position, ActiveFielder> = HashMap::new();

    let p = ActiveFielder {
        position: Position::P,
        player_id: 0,
        info: FielderInfo {
            fielder_type: FielderType::Pitcher,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
        polar_position: PolarPosition::new(MOUND_DISTANCE, 0.0),
    };

    let c = ActiveFielder {
        position: Position::C,
        player_id: 1,
        info: FielderInfo {
            fielder_type: FielderType::Catcher,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
        polar_position: PolarPosition::new(0.0, 0.0),
    };

    let fb = ActiveFielder {
        position: Position::FB,
        player_id: 2,
        info: FielderInfo {
            fielder_type: FielderType::CornerInfielder,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
        polar_position: PolarPosition::new(35.0, 33.0),
    };

    let sb = ActiveFielder {
        position: Position::SB,
        player_id: 3,
        info: FielderInfo {
            fielder_type: FielderType::MiddleInfielder,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
        polar_position: PolarPosition::new(40.0, 18.0),
    };

    let tb = ActiveFielder {
        position: Position::TB,
        player_id: 4,
        info: FielderInfo {
            fielder_type: FielderType::CornerInfielder,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
        polar_position: PolarPosition::new(35.0, -33.0),
    };

    let ss = ActiveFielder {
        position: Position::SS,
        player_id: 5,
        info: FielderInfo {
            fielder_type: FielderType::MiddleInfielder,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
        polar_position: PolarPosition::new(40.0, -18.0),
    };

    let rf = ActiveFielder {
        position: Position::RF,
        player_id: 6,
        info: FielderInfo {
            fielder_type: FielderType::Outfielder,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
        polar_position: PolarPosition::new(80.0, 26.0),
    };

    let cf = ActiveFielder {
        position: Position::CF,
        player_id: 7,
        info: FielderInfo {
            fielder_type: FielderType::Outfielder,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
        polar_position: PolarPosition::new(90.0, 0.0),
    };

    let lf = ActiveFielder {
        position: Position::LF,
        player_id: 8,
        info: FielderInfo {
            fielder_type: FielderType::Outfielder,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
        polar_position: PolarPosition::new(80.0, -26.0),
    };

    [p, c, fb, sb, tb, ss, rf, cf, lf]
}

// TODO: Retrieve from PlayerFactory
fn generate_random_batter() -> BatterInfo {
    let mut rng = rand::rng();

    let roll_rl = rng.random_range(0.0..1.0);
    let weight_left = 0.3;

    let mut rl = RL::Right;
    if roll_rl < weight_left {
        rl = RL::Left;
    }

    let min_swing_speed = 110.0;
    let max_swing_speed = 150.0;

    // TODO: Change mean by batter type
    let mean = (min_swing_speed + max_swing_speed) * 0.5;
    let std_dev = (max_swing_speed - min_swing_speed) / 6.0;
    let final_swing_speed = (mean + std_dev * rng.sample::<f64, _>(StandardNormal))
        .clamp(min_swing_speed, max_swing_speed);

    let pull_hitter = BatterInfo {
        batting_side: rl,
        swing_speed: final_swing_speed,
        weight_pull: 0.55,
        weight_center: 0.25,
        weight_opposite: 0.05,
        weight_foul_left: 0.12,
        weight_foul_right: 0.03,
    };

    let ordinally_hitter = BatterInfo {
        batting_side: rl,
        swing_speed: final_swing_speed,
        weight_pull: 0.35,
        weight_center: 0.35,
        weight_opposite: 0.15,
        weight_foul_left: 0.08,
        weight_foul_right: 0.07,
    };

    let average_hitter = BatterInfo {
        batting_side: rl,
        swing_speed: final_swing_speed,
        weight_pull: 0.25,
        weight_center: 0.45,
        weight_opposite: 0.25,
        weight_foul_left: 0.02,
        weight_foul_right: 0.03,
    };

    let weight_pull_hitter = 0.3;
    let weight_ordinally_hitter = 0.5;
    let weight_average_hitter = 0.2;

    let total_hitter_weight = weight_pull_hitter + weight_ordinally_hitter + weight_average_hitter;
    let mut roll_hitter = rng.random_range(0.0..total_hitter_weight);

    if roll_hitter < weight_pull_hitter {
        return pull_hitter;
    }
    roll_hitter -= weight_pull_hitter;

    if roll_hitter < weight_ordinally_hitter {
        return ordinally_hitter;
    }
    return average_hitter;
}

#[test]
fn test_through_inning() -> Result<(), GameError> {
    let stadium = generate_stadium();
    let batter = generate_random_batter();
    let fielders = generate_default_fielders();
    let pitcher = PitcherInfo {
        pitcher_style: PitcherStyle::BalancedPitcher,
        velocity: 0.0,
        control: 0.0,
        stamina: 0.0,
        injury_proneness: 0.0,
        clutch: 0.0,
        hpp: 0.0,
        platoon_splitting: 0.0,
        delivery_motion_time: 2.0,
        pitch_skills: vec![],
        fielder_info: FielderInfo {
            fielder_type: FielderType::Pitcher,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
    };

    let catcher = CatcherInfo {
        fielder_info: FielderInfo {
            fielder_type: FielderType::Catcher,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
    };

    let mut scores = 0;

    let batter_runner = ActiveRunner {
        player_id: 0,
        skills: RunningSkills {
            speed: 7.0,
            lead_distance: 0.0,
            start_reaction: 0.1,
        },
    };

    let mut inning_state = InningState::new();

    while let InningProgress::Ongoing = inning_state.progress() {
        println!("\n--- New count ---");
        inning_state.runners.batting_side = Some(batter.batting_side);
        inning_state.runners.batter_runner = Some(batter_runner);

        let ball = calculate_batted_ball(&batter, 150.0);

        println!("{:#?}", ball);

        if stadium.is_stand_in(&ball) {
            if ball.is_foul() {
                println!("{}", BattingResult::Foul);
                println!("Outs:{}, Scores:{}", inning_state.out, scores);
                continue;
            } else {
                scores += inning_state.runners.after_homerun();

                println!("{}, score:+{}", BattingResult::HomeRun, scores);
                println!("Outs:{}, Scores:{}", inning_state.out, scores);
                continue;
            }
        }

        let fielder = {
            let handler = process_defensive_chain(&fielders, &ball)?;
            handler.fielder
        };

        println!("{:#?}", fielder);

        let fielded_ball = fielder.try_catch(&ball);

        println!("{:#?}", fielded_ball);

        if fielded_ball.is_fly_catch {
            inning_state.add_out();

            println!(
                "Fly is caught. Outs:{}, Scores:{}",
                inning_state.out, scores
            );

            // TODO: Record fielding result

            if fielded_ball.fielded_by.is_infielder() || !inning_state.allows_tagup() {
                println!("No tag-up.");
                continue;
            }
        }

        // TODO: stolen base tunrned into hit-and-run case
        if inning_state.can_steal_base(Box::new(RealRng::new())) {
            let steal_defense_play_result =
                evaluate_base_stealing(Base::Second, &pitcher, &catcher, Box::new(RealRng::new()));

            println!("{:#?}", steal_defense_play_result);

            let steal_runner_advance_result = inning_state
                .runners
                .after_base_stealing(steal_defense_play_result)?;

            println!("{:#?}", steal_runner_advance_result);

            // TODO: Record stolen base result

            if steal_runner_advance_result.ruling == Ruling::Out {
                inning_state.add_out();
                if inning_state.progress() == InningProgress::HalfInningOver {
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

        let defense_play_result = evaluate_defense_play(&ctx, Box::new(RealRng::new()))?;

        println!("{:#?}", defense_play_result);

        let runner_advance_result = if ctx.fielded_ball.fielded_by.is_outfielder() {
            if inning_state.allows_tagup() {
                inning_state.runners.after_tagup(&defense_play_result)?
            } else {
                inning_state
                    .runners
                    .after_outfield_hit(&defense_play_result)?
            }
        } else {
            inning_state
                .runners
                .after_infield_grounder(&defense_play_result)?
        };

        println!("{:#?}", runner_advance_result);

        if ctx.fielded_ball.fielded_by.is_infielder() && inning_state.can_double_play() {
            if let Some(double_play_defense_play_result) =
                evaluate_double_play(&ctx, &defense_play_result, Box::new(RealRng::new()))?
            {
                println!("{:#?}", double_play_defense_play_result);

                let double_play_runner_advance_result = inning_state.runners.after_double_play(
                    double_play_defense_play_result,
                    runner_advance_result.unsaved_runners,
                )?;

                println!("{:#?}", double_play_runner_advance_result);

                inning_state
                    .runners
                    .commit_unsaved_runners(double_play_runner_advance_result.unsaved_runners);

                if double_play_runner_advance_result.ruling == Ruling::Out {
                    inning_state.add_out();
                    if inning_state.progress() == InningProgress::Ongoing {
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

        // TODO: Record fielding result
    }

    Ok(())
}

#[test]
fn test_inning_double_play_deterministically() -> Result<(), GameError> {
    let fielders = generate_default_fielders();

    let batter_runner = ActiveRunner {
        player_id: 0,
        skills: RunningSkills {
            speed: 7.0,
            lead_distance: 0.0,
            start_reaction: 0.1,
        },
    };

    let runner_on_first = ActiveRunner {
        player_id: 1,
        skills: RunningSkills {
            speed: 7.0,
            lead_distance: 0.0,
            start_reaction: 0.1,
        },
    };

    let mut inning_state = InningState::new();
    inning_state.runners.batting_side = Some(RL::Left);
    inning_state.runners.batter_runner = Some(batter_runner);
    inning_state.runners.runner_1st = Some(runner_on_first);

    let ball = BattedBall::new(95.0, 4.0, -25.0, 35.0, 1.0, TrajectoryType::Grounder);

    let fielder = fielders.iter().find(|f| f.is(Position::SS)).unwrap();
    let fielded_ball = FieldedBall {
        ball,
        fielded_by: Position::SS,
        time_to_field: 1.0,
        is_fly_catch: false,
    };

    let ctx = PlayContext {
        runners: &inning_state.runners,
        fielders: &fielders,
        try_catch_fielder: fielder,
        fielded_ball: &fielded_ball,
    };

    let first_play = evaluate_defense_play(&ctx, Box::new(FixedRng::new(0.1)))?;
    let first_advance = inning_state.runners.after_infield_grounder(&first_play)?;

    assert_eq!(first_advance.ruling, Ruling::Out);

    let second_play =
        evaluate_double_play(&ctx, &first_play, Box::new(FixedRng::new(0.1)))?.unwrap();

    inning_state.add_out();

    let second_advance = inning_state
        .runners
        .after_double_play(second_play, first_advance.unsaved_runners)?;

    assert_eq!(second_advance.ruling, Ruling::Out);
    inning_state.add_out();

    assert_eq!(inning_state.out, 2);

    Ok(())
}

#[test]
fn test_inning_base_steal_deterministically() -> Result<(), GameError> {
    let pitcher = PitcherInfo {
        pitcher_style: PitcherStyle::BalancedPitcher,
        velocity: 0.0,
        control: 0.0,
        stamina: 0.0,
        injury_proneness: 0.0,
        clutch: 0.0,
        hpp: 0.0,
        platoon_splitting: 0.0,
        delivery_motion_time: 2.0,
        pitch_skills: vec![],
        fielder_info: FielderInfo {
            fielder_type: FielderType::Pitcher,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
    };
    let catcher = CatcherInfo {
        fielder_info: FielderInfo {
            fielder_type: FielderType::Catcher,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
        },
    };
    let runner_on_first = ActiveRunner {
        player_id: 0,
        skills: RunningSkills {
            speed: 7.0,
            lead_distance: 0.0,
            start_reaction: 0.1,
        },
    };

    let mut inning_state = InningState::new();
    inning_state.runners.runner_1st = Some(runner_on_first);

    assert!(inning_state.can_steal_base(Box::new(FixedRng::new(0.1))));

    let steal_defense_play_result = evaluate_base_stealing(
        Base::Second,
        &pitcher,
        &catcher,
        Box::new(FixedRng::new(0.1)),
    );
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
fn test_draw_stadium() {
    let stadium = generate_stadium();

    stadium.draw_fence();
}

#[test]
fn test_base_running() {
    let arraival_time = BASE_DISTANCE / 7.7;
    println!("{}", arraival_time);
}

#[test]
fn test_catch_batted_ball() {
    let conn = SqlDb::new().unwrap().get_conn().unwrap();

    let right_average_hitter = BatterInfo {
        batting_side: RL::Right,
        swing_speed: 125.0,
        weight_pull: 0.35,
        weight_center: 0.35,
        weight_opposite: 0.15,
        weight_foul_left: 0.08,
        weight_foul_right: 0.07,
    };

    for _ in 0..1000 {
        let ball = calculate_batted_ball(&right_average_hitter, 150.0);
        conn.execute(
            "INSERT INTO test_batted_ball (launch_speed_kmh, launch_angle,  spray_angle, distance, hang_time, trajectory) 
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![ball.launch_speed_kmh, ball.launch_angle, ball.angle(), ball.distance(),  ball.hang_time, ball.trajectory.as_ref()],
        )
        .unwrap();
    }
}
