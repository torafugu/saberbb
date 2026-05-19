DROP TABLE count;

CREATE TABLE count (
    game_id INTEGER,
    inning_seq INTEGER,
    inning_tb TEXT,
    seq INTEGER,
    bases_occupied INTEGER NOT NULL DEFAULT 0,
    pitcher_id INTEGER,
    catcher_id INTEGER,
    first_baseman_id INTEGER,
    second_baseman_id INTEGER,
    third_baseman_id INTEGER,
    shortstop_id INTEGER,
    left_fielder_id INTEGER,
    center_fielder_id INTEGER,
    right_fielder_id INTEGER,
    batter_id INTEGER,
    result TEXT NOT NULL,
    point INTEGER NOT NULL,
    out INTEGER NOT NULL,
    PRIMARY KEY (
        game_id,
        inning_seq,
        inning_tb,
        seq
    )
);