DROP TABLE batted_ball;

CREATE TABLE batted_ball (
    game_id INTEGER,
    count_seq INTEGER,
    launch_speed_kmh REAL NOT NULL,
    launch_angle REAL NOT NULL,
    polar_distance REAL NOT NULL,
    polar_angle REAL NOT NULL,
    hang_time REAL NOT NULL,
    trajectory TEXT NOT NULL,
    PRIMARY KEY (game_id, count_seq)
);