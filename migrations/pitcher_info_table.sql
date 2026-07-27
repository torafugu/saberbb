DROP TABLE pitcher_info;

CREATE TABLE pitcher_info(
    player_id INTEGER PRIMARY KEY,
    throw_side TEXT NOT NULL,
    arm_slot TEXT NOT NULL,
    pitcher_style TEXT NOT NULL,
    velocity REAL NOT NULL,
    control REAL NOT NULL,
    stamina REAL NOT NULL,
    injury_proneness REAL NOT NULL,
    clutch REAL NOT NULL,
    hpp REAL NOT NULL,
    platoon_splitting REAL NOT NULL,
    delivery_motion_time REAL NOT NULL
);
