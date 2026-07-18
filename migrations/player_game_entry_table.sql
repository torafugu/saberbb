DROP TABLE player_game_entry;

CREATE TABLE player_game_entry (
    game_id INTEGER,
    start_count_seq INTEGER,
    end_count_seq INTEGER,
    position TEXT,
    player_id INTEGER,
    PRIMARY KEY (
        game_id,
        start_count_seq,
        player_id
    )
);