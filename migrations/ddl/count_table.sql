DROP TABLE count;

CREATE TABLE count (
    game_id INTEGER,
    inning_seq INTEGER,
    inning_tb TEXT,
    seq INTEGER,
    point INTEGER NOT NULL,
    ball INTEGER NOT NULL,
    strike INTEGER NOT NULL,
    out INTEGER NOT NULL,
    PRIMARY KEY (
        game_id,
        inning_seq,
        inning_tb,
        seq
    )
);