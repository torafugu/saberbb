DROP TABLE batter_info;

CREATE TABLE batter_info (
    player_id INTEGER PRIMARY KEY,
    batting_side TEXT NOT NULL,
    batter_type TEXT NOT NULL,
    zone_aptitude TEXT NOT NULL,
    hot_zone_scale REAL NOT NULL,
    batting_eye REAL NOT NULL,
    swing_speed REAL NOT NULL,
    swing_power REAL NOT NULL,
    attack_angle REAL NOT NULL,
    bat_control REAL NOT NULL,
    consistency REAL NOT NULL
);