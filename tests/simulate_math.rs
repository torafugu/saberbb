use rusqlite::params;
use saberbb::domain::random_provider::*;
use saberbb::domain::util::*;
use saberbb::repositories::db::*;

#[test]
fn test_normal_random() {
    let conn = SqlDb::new().unwrap().get_conn().unwrap();
    conn.execute("DELETE FROM test_normal_random", []).unwrap();

    let mut rng = RealRng::new();

    for _ in 0..1000 {
        let value = rng.normal_random(0.0, 0.01, 0.0, 1.0, 0.0);

        conn.execute(
            "INSERT INTO test_normal_random (value) VALUES (?1)",
            params![value],
        )
        .unwrap();
    }
}

#[test]
fn test_optimal_angle() {
    let x_m: f64 = 0.1;
    let z_m: f64 = 0.2;

    let deg = z_m.atan2(x_m).to_degrees();

    println!("bat_angle_deg:{}", deg);
}

#[test]
fn test_softmax() {
    let value = softmax(&vec![0.1069, 0.1222]);

    println!("softmax:{:?}", value);
}

#[test]
fn test_sigmoid() {
    let value = sigmoid(-0.018);

    println!("sigmoid:{}", value);
}
