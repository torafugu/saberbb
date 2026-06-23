use rand::RngExt;
use rand_distr::StandardNormal;
use rusqlite::params;
use saberbb::domain::resolver::batting_resolver::*;
use saberbb::domain::resolver::fielding_resolver::*;
use saberbb::domain::shared::ball::Ball;
use saberbb::domain::shared::ball::TrajectoryType;
use saberbb::domain::shared::game::*;
use saberbb::domain::shared::game_state::*;
use saberbb::domain::shared::player::*;
use saberbb::domain::shared::stadium::*;
use saberbb::repositories::db::*;

fn generate_stadium() -> Stadium {
    Stadium::new("AAA".to_string(), 98.0, 120.0, 2.0)
}

fn generate_default_fielders() -> [Fielder; 9] {
    // TODO: Randomize throw_speed, running_speed, reaction and prep_time
    let p = Fielder::new(Position::P, MOUND_DISTANCE, 0.0, 40.0, 7.0, 0.5, 0.65);
    let c = Fielder::new(Position::C, 0.0, 0.0, 40.0, 7.0, 0.5, 0.65);
    let fb = Fielder::new(Position::FB, 35.0, 33.0, 40.0, 7.0, 0.5, 0.65);
    let sb = Fielder::new(Position::SB, 40.0, 18.0, 40.0, 7.0, 0.5, 0.65);
    let tb = Fielder::new(Position::TB, 35.0, -33.0, 40.0, 7.0, 0.5, 0.65);
    let ss = Fielder::new(Position::SS, 35.0, -33.0, 40.0, 7.0, 0.5, 0.65);
    let rf = Fielder::new(Position::RF, 80.0, 26.0, 40.0, 7.0, 0.5, 0.65);
    let cf = Fielder::new(Position::CF, 90.0, 0.0, 40.0, 7.0, 0.5, 0.65);
    let lf = Fielder::new(Position::LF, 80.0, -26.0, 40.0, 7.0, 0.5, 0.65);

    [p, c, fb, sb, tb, ss, rf, cf, lf]
}

fn generate_random_batter() -> Batter {
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

    let pull_hitter = Batter {
        batting_side: rl.clone(),
        swing_speed: final_swing_speed,
        weight_pull: 0.55,
        weight_center: 0.25,
        weight_opposite: 0.05,
        weight_foul_left: 0.12,
        weight_foul_right: 0.03,
    };

    let ordinally_hitter = Batter {
        batting_side: rl.clone(),
        swing_speed: final_swing_speed,
        weight_pull: 0.35,
        weight_center: 0.35,
        weight_opposite: 0.15,
        weight_foul_left: 0.08,
        weight_foul_right: 0.07,
    };

    let average_hitter = Batter {
        batting_side: rl.clone(),
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
fn test_bat_to_catch() -> Result<(), GameError> {
    let stadium = generate_stadium();

    let batter = generate_random_batter();
    println!(
        "Batter:{}, Swing Speed:{}",
        batter.batting_side, batter.swing_speed
    );

    let mut ball = calculate_batted_ball(&batter, 150.0);

    println!(
        "Ball?:(Degree:{},Distance:{}, TrajectoryType:{})",
        ball.angle(),
        ball.distance(),
        ball.trajectory
    );

    if stadium.is_stand_in(&ball) {
        if ball.is_foul() {
            println!("{}", BattingResult::Foul);
        } else {
            println!("{}", BattingResult::HomeRun);
        }
    }

    let fielders = generate_default_fielders();
    let fielder = {
        let handler = process_defensive_chain(&fielders, &mut ball)?;

        println!(
            "Who?:{}, Ball arrival time:{}, TrajectoryType:{}",
            handler.fielder.position, handler.ball.hang_time, handler.ball.trajectory
        );

        handler.fielder
    };

    let catch_result = fielder.try_catch(&mut ball);

    println!(
        "time_to_catch?:{}, final_distance?:{}, angle?:{}, is_fly_catch?:{}",
        catch_result.time_to_catch,
        catch_result.ball.distance(),
        catch_result.ball.angle(),
        catch_result.is_fly_catch,
    );

    if !catch_result.is_fly_catch {
        println!("Play Result:{}", Ruling::Out);
    }

    let runners = RunnersOnBase {
        batter_runner: Runner {
            speed: 7.0,
            lead_distance: 0.0,
        },
        runner_1st: None,
        runner_2nd: None,
        runner_3rd: None,
    };

    let ctx = PlayContext {
        runners: &runners,
        fielders: &fielders,
        try_catch_fielder: fielder,
        ball: catch_result.ball,
        time_to_catch: catch_result.time_to_catch,
        is_fly_catch: catch_result.is_fly_catch,
    };

    let play_result = evaluate_defense_play(&ctx, batter.batting_side)?;

    println!(
        "ruling?:{}, defense_time?:{}, runner_time?:{}, time_difference?:{}",
        play_result.ruling,
        play_result.defense_time,
        play_result.runner_time,
        play_result.time_difference
    );

    Ok(())
}

#[test]
fn test_stand_in() {
    let stadium = generate_stadium();
    let ball = Ball::new(170.0, 35.0, 30.0, 130.0, 5.0, TrajectoryType::Fly);
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
    let ball = Ball::new(160.0, 35.0, 30.0, 300.0, 5.0, TrajectoryType::Fly);
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

    let right_average_hitter = Batter {
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
