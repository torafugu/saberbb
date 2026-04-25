DROP TABLE inning;

CREATE TABLE inning (
    game_round_season INTEGER,
    game_round_seq INTEGER,
    game_seq INTEGER,
    seq INTEGER,
    tb TEXT,
    point INTEGER NOT NULL,
    PRIMARY KEY (
        game_round_season,
        game_round_seq,
        game_seq,
        seq,
        tb
    )
);