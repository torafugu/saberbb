DROP TABLE defensive_skill;

CREATE TABLE defensive_skill (
    player_id INTEGER,
    position TEXT,
    mod_uzr REAL NOT NULL,
    PRIMARY KEY (player_id, position)
);