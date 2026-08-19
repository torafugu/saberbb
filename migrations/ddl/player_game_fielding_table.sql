DROP TABLE player_game_fielding;

CREATE TABLE player_game_fielding (
    game_id INTEGER,
    count_seq INTEGER,
    seq INTEGER,
    catch_fielder_id INTEGER NOT NULL,
    catch_fielder_position TEXT NOT NULL,
    cutoff_fielder_id INTEGER,
    cutoff_fielder_position TEXT,
    final_fielder_id INTEGER,
    final_fielder_position TEXT,
    time_to_field REAL NOT NULL,
    play_type TEXT NOT NULL,
    PRIMARY KEY (game_id, count_seq, seq)
);