DROP TABLE inning;

CREATE TABLE inning (
    game_id INTEGER,
    seq INTEGER,
    tb TEXT,
    point INTEGER NOT NULL,
    PRIMARY KEY (game_id, seq, tb)
);