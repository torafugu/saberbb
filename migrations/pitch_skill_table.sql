DROP TABLE pitch_skill;

CREATE TABLE pitch_skill (
    player_id INTEGER,
    pitch_type TEXT,
    velocity REAL NOT NULL,
    control REAL NOT NULL,
    stamina REAL NOT NULL,
    injury_proneness REAL NOT NULL,
    stuff REAL NOT NULL,
    fb REAL NOT NULL,
    gp REAL NOT NULL,
    horizontal_movement REAL NOT NULL,
    vertical_movement REAL NOT NULL,
    spin_rate REAL NOT NULL,
    usage REAL NOT NULL,
    PRIMARY KEY (player_id, pitch_type)
);