DROP TABLE batter_info;

CREATE TABLE batter_info (
    player_id INTEGER PRIMARY KEY,
    batting_side TEXT NOT NULL,
    batting_eye REAL NOT NULL,
    swing_speed REAL NOT NULL,
    swing_power REAL NOT NULL,
    attack_angle REAL NOT NULL,
    bat_contact REAL NOT NULL,
    timing_bias REAL NOT NULL,
    consistency_sigma REAL NOT NULL
);