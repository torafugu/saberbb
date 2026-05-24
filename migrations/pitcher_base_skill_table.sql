DROP TABLE pitcher_base_skill;

CREATE TABLE pitcher_base_skill (
    player_id INTEGER PRIMARY KEY,
    mod_velocity REAL NOT NULL,
    mod_control REAL NOT NULL,
    mod_stamina REAL NOT NULL,
    mod_injury_proneness REAL NOT NULL,
    mod_clutch REAL NOT NULL,
    mod_hpp REAL NOT NULL,
    mod_platoon_splitting REAL NOT NULL
);