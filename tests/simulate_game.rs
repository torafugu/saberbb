use kurbo::Point;
use saberbb::domain::resolver::*;
use saberbb::domain::shared::ball::*;
use saberbb::domain::shared::game::*;
use saberbb::domain::shared::player::*;
use saberbb::domain::shared::stadium::*;
use saberbb::repositories::db::*;

pub fn gennerate_default_fielders() -> [Fielder; 9] {
    let p = Fielder {
        position: Position::P,
        distance: MOUND_DISTANCE,
        angle: 0.0,
        speed: 7.0,
        reaction: 0.5,
    };

    let c = Fielder {
        position: Position::C,
        distance: 0.0,
        angle: 0.0,
        speed: 7.0,
        reaction: 0.5,
    };

    let fb = Fielder {
        position: Position::FB,
        distance: 35.0,
        angle: 33.0,
        speed: 7.0,
        reaction: 0.5,
    };

    let sb = Fielder {
        position: Position::SB,
        distance: 40.0,
        angle: 18.0,
        speed: 7.0,
        reaction: 0.5,
    };

    let tb = Fielder {
        position: Position::TB,
        distance: 35.0,
        angle: -33.0,
        speed: 7.0,
        reaction: 0.5,
    };

    let ss = Fielder {
        position: Position::SS,
        distance: 40.0,
        angle: -18.0,
        speed: 7.0,
        reaction: 0.5,
    };

    let rf = Fielder {
        position: Position::RF,
        distance: 80.0,
        angle: 26.0,
        speed: 7.0,
        reaction: 0.5,
    };

    let cf = Fielder {
        position: Position::CF,
        distance: 90.0,
        angle: 0.0,
        speed: 7.0,
        reaction: 0.5,
    };

    let lf = Fielder {
        position: Position::LF,
        distance: 80.0,
        angle: -26.0,
        speed: 7.0,
        reaction: 0.5,
    };

    [p, c, fb, sb, tb, ss, rf, cf, lf]
}

#[test]
fn test_bat_to_catch() {
    let right_average_hitter = Batter {
        batting_side: RL::Right,
        swing_speed: 150.0,
        weight_pull: 0.35,
        weight_center: 0.35,
        weight_opposite: 0.15,
        weight_foul_left: 0.08,
        weight_foul_right: 0.07,
    };

    let ball = calculate_batted_ball(&right_average_hitter, 150.0);

    println!(
        "Ball?:(Degree:{},Distance:{}, TrajectoryType:{})",
        ball.spray_angle, ball.distance, ball.trajectory
    );

    let handler = find_closest_fielder(&gennerate_default_fielders(), &ball);

    println!("Who?:{}", handler.position);
    println!("Catch?:{}", handler.try_catch(&ball));
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
            [ball.distance, ball.spray_angle, ball.hang_time],
        )
        .unwrap();
    }
}
