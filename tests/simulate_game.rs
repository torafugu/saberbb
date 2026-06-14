use kurbo::Point;
use saberbb::domain::resolver::Batter;
use saberbb::domain::resolver::*;
use saberbb::domain::shared::game::*;
use saberbb::domain::shared::player::RL;
use saberbb::domain::shared::stadium::*;
use saberbb::repositories::db::*;

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
