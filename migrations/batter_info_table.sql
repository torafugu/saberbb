DROP TABLE batter_info;

CREATE TABLE batter_info (
    player_id INTEGER PRIMARY KEY,
    batting_side TEXT NOT NULL,
    swing_speed REAL NOT NULL,
    base_launch_angle REAL NOT NULL,
    consistency_sigma REAL NOT NULL,
    weight_foul_pull REAL NOT NULL,
    weight_pull REAL NOT NULL,
    weight_center REAL NOT NULL,
    weight_opposite REAL NOT NULL,
    weight_foul_opposite REAL NOT NULL
);
