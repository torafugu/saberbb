DROP TABLE game_round;

CREATE TABLE game_round (
    id INTEGER,
    season INTEGER,
    seq INTEGER,
    date TEXT NOT NULL,
    PRIMARY KEY (id)
);