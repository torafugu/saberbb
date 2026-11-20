CREATE TABLE IF NOT EXISTS game_pitcher_workload (
    game_id INTEGER NOT NULL,
    player_id INTEGER NOT NULL,
    pitches INTEGER NOT NULL CHECK(pitches > 0),
    reference_pitches REAL NOT NULL CHECK(reference_pitches > 0),
    PRIMARY KEY (game_id, player_id)
);
CREATE INDEX IF NOT EXISTS game_pitcher_workload_player ON game_pitcher_workload(player_id);
