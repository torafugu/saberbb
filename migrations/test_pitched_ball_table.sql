DROP TABLE test_pitched_ball;

CREATE TABLE test_pitched_ball (
    pitch_type TEXT NOT NULL,
    speed_kmh REAL NOT NULL,
    spin_rate REAL NOT NULL,
    spin_angle REAL NOT NULL,
    spin_efficiency REAL NOT NULL,
    release_point_x REAL NOT NULL,
    release_point_y REAL NOT NULL,
    release_point_z REAL NOT NULL,
    flight_time REAL NOT NULL,
    target_location TEXT NOT NULL,
    aim_x REAL NOT NULL,
    aim_y REAL NOT NULL,
    norm_x REAL NOT NULL,
    norm_y REAL NOT NULL
);