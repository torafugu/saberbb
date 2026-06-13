use saberbb::domain::resolver::*;
use saberbb::domain::shared::game::*;
use saberbb::domain::shared::stadium::*;
use saberbb::repositories::db::*;

#[test]
fn test_draw_stadium() {
    draw(generate_svg());
}

#[test]
fn test_base_running() {
    let arraival_time = BASE_DISTANCE / 7.7;
    println!("{}", arraival_time);
}

#[test]
fn test_catch_batted_ball() {
    let conn = SqlDb::new().unwrap().get_conn().unwrap();

    for _ in 0..1000 {
        let ball = calculate_batted_ball(150.0, 150.0);
        conn.execute(
            "INSERT INTO test_batted_ball (distance, spray_angle, hang_time) VALUES (?1, ?2, ?3)",
            [ball.distance, ball.spray_angle, ball.hang_time],
        )
        .unwrap();
    }
}
