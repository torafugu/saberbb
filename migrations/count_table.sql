DROP TABLE count;

CREATE TABLE count (
    game_round_season INTEGER,
    game_round_seq INTEGER,
    game_seq INTEGER,
    inning_seq INTEGER,
    inning_tb TEXT,
    seq INTEGER,
    is_first_runner BOOLEAN NOT NULL DEFAULT 0,
    is_second_runner BOOLEAN NOT NULL DEFAULT 0,
    is_third_runner BOOLEAN NOT NULL DEFAULT 0,
    batter_id INTEGER,
    result TEXT NOT NULL,
    point INTEGER NOT NULL,
    out INTEGER NOT NULL,
    PRIMARY KEY (
        game_round_season,
        game_round_seq,
        game_seq,
        inning_seq,
        inning_tb,
        seq
    )
);