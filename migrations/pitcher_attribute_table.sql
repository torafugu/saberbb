DROP TABLE pitcher_attribute;

CREATE TABLE pitcher_attribute (
    player_id INTEGER,
    pitcher_style TEXT,
    mod_velocity REAL NOT NULL,
    mod_control REAL NOT NULL,
    mod_stamina REAL NOT NULL,
    mod_injury_proneness REAL NOT NULL,
    mod_clutch REAL NOT NULL,
    mod_hpp REAL NOT NULL,
    mod_platoon_splitting REAL NOT NULL,
    PRIMARY KEY (player_id, pitcher_style)
);