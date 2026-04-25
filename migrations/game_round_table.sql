DROP TABLE game_round;

CREATE TABLE game_round (
    season INTEGER,
    seq INTEGER,
    date TEXT NOT NULL,
    PRIMARY KEY (season, seq)
);