DROP TABLE batting_order_history;

CREATE TABLE batting_order_history (
    game_id INTEGER,
    start_inning_seq INTEGER,
    start_inning_tb TEXT,
    start_count_seq INTEGER,
    end_inning_seq INTEGER,
    end_inning_tb TEXT,
    end_count_seq INTEGER,
    team_id INTEGER,
    index_num INTEGER,
    position TEXT,
    player_id INTEGER,
    PRIMARY KEY (
        game_id,
        start_inning_seq,
        start_inning_tb,
        start_count_seq,
        index_num
    )
);
