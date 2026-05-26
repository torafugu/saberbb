DROP TABLE pitch_skill;

CREATE TABLE pitch_skill (
    player_id INTEGER,
    pitch_type TEXT,
    mod_velocity REAL NOT NULL,
    mod_control REAL NOT NULL,
    mod_stamina REAL NOT NULL,
    mod_injury_proneness REAL NOT NULL,
    mod_stuff REAL NOT NULL,
    mod_fb REAL NOT NULL,
    mod_gp REAL NOT NULL,
    mod_horizontal_movement REAL NOT NULL,
    mod_vertical_movement REAL NOT NULL,
    mod_spin_rate REAL NOT NULL,
    mod_usage REAL NOT NULL,
    PRIMARY KEY (player_id, pitch_type)
);