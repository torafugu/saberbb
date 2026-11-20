CREATE TABLE IF NOT EXISTS game_position_player_workload (
    game_id INTEGER NOT NULL,
    player_id INTEGER NOT NULL,
    fielding REAL NOT NULL CHECK(fielding >= 0),
    running REAL NOT NULL CHECK(running >= 0),
    batting REAL NOT NULL CHECK(batting >= 0),
    PRIMARY KEY (game_id, player_id)
);
CREATE INDEX IF NOT EXISTS game_position_player_workload_player ON game_position_player_workload(player_id);
