DROP TABLE pitch_skill;

CREATE TABLE pitch_skill (
    player_id INTEGER,
    pitch_type TEXT,
    velocity REAL NOT NULL,
    control REAL NOT NULL,
    stamina REAL NOT NULL,
    injury_proneness REAL NOT NULL,
    spin_rate REAL NOT NULL,
    spin_angle REAL NOT NULL,
    spin_efficiency REAL NOT NULL,
    usage REAL NOT NULL,
    PRIMARY KEY (player_id, pitch_type)
);
