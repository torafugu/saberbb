DROP TABLE active_fielder_history;

CREATE TABLE active_fielder_history (
    game_id INTEGER,
    start_count_seq INTEGER,
    end_count_seq INTEGER,
    team_id INTEGER,
    position TEXT,
    player_id INTEGER,
    PRIMARY KEY (
        game_id,
        start_count_seq,
        position
    )
);