DROP TABLE batting_result_history;

CREATE TABLE batting_result_history (
    game_id INTEGER,
    inning_seq INTEGER,
    inning_tb TEXT,
    count_seq INTEGER,
    team_id INTEGER,
    pitcher_id INTEGER,
    batter_id INTEGER,
    result TEXT NOT NULL,
    PRIMARY KEY (
        game_id,
        inning_seq,
        inning_tb,
        count_seq,
        batter_id
    )
);