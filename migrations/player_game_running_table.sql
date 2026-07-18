DROP TABLE player_game_running;

CREATE TABLE player_game_running (
    game_id INTEGER,
    count_seq INTEGER,
    seq INTEGER,
    defense_time REAL NOT NULL,
    runner_time REAL NOT NULL,
    throw_target_base TEXT NOT NULL,
    play_type TEXT NOT NULL,
    ruling TEXT NOT NULL,
    runs_scored INTEGER NOT NULL,
    runner_1st_id INTEGER,
    runner_2nd_id INTEGER,
    runner_3rd_id INTEGER,
    PRIMARY KEY (game_id, count_seq, seq)
);