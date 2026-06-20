use kurbo::Point;
use rand::RngExt;
use rand_distr::StandardNormal;
use saberbb::domain::resolver::batting_resolver::*;
use saberbb::domain::resolver::fielding_resolver::*;
use saberbb::domain::shared::game::*;
use saberbb::domain::shared::game_state::*;
use saberbb::domain::shared::player::*;
use saberbb::domain::shared::stadium::*;
use saberbb::repositories::db::*;

pub fn gennerate_default_fielders() -> [Fielder; 9] {
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

pub fn gennerate_random_batter() -> Batter {
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
fn test_bat_to_catch() {
    let batter = gennerate_random_batter();
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

    let fielders = gennerate_default_fielders();
    let fielder = {
        let handler = process_defensive_chain(&fielders, &mut ball);

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

    // in case ruling is out, skip following
    if !catch_result.is_fly_catch {
        let runners = RunnersOnBase {
            batter_speed: 7.0,
            runner_1st_speed: Some(7.0),
            runner_2nd_speed: None,
            runner_3rd_speed: None,
        };

        let ctx = PlayContext {
            bases_occupied: RUNNER_1ST,
            fielder: fielder,
            ball: catch_result.ball,
            time_to_catch: catch_result.time_to_catch,
        };

        let play_result = evaluate_defense_play(&ctx, &runners, 1.0, batter.batting_side);

        println!(
            "ruling?:{}, defense_time?:{}, runner_time?:{}, time_difference?:{}",
            play_result.ruling,
            play_result.defense_time,
            play_result.runner_time,
            play_result.time_difference
        );
    } else {
        println!("Play Result:{}", Ruling::Out);
    }

    // &ball.polar_position.distance = catch_result.final_position;

    // let result = evaluate_throw_play(evaluate_throw_play, handler, );
}

#[test]
fn test_stand_in() {
    let stadium = Stadium::new("AAA".to_string(), 98.0, 120.0);

    if stadium.is_inside_fence_line(Point::new(65.0, 70.0)) {
        println!("In ground !"); // Hit, Direct hit on the fence
    } else {
        println!("Stand In !");
    }
}

#[test]
fn test_draw_stadium() {
    let stadium = Stadium::new("AAA".to_string(), 98.0, 120.0);

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
        swing_speed: 150.0,
        weight_pull: 0.35,
        weight_center: 0.35,
        weight_opposite: 0.15,
        weight_foul_left: 0.08,
        weight_foul_right: 0.07,
    };

    for _ in 0..1000 {
        let ball = calculate_batted_ball(&right_average_hitter, 150.0);
        conn.execute(
            "INSERT INTO test_batted_ball (distance, spray_angle, hang_time) VALUES (?1, ?2, ?3)",
            [ball.distance(), ball.angle(), ball.hang_time],
        )
        .unwrap();
    }
}
