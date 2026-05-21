DROP TABLE inning;

CREATE TABLE inning (
    game_id INTEGER,
    seq INTEGER,
    tb TEXT,
    PRIMARY KEY (game_id, seq, tb)
);