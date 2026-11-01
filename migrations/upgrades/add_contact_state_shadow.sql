-- Diagnostic-only snapshots. Apply once to existing databases before generating games.
CREATE TABLE IF NOT EXISTS player_game_contact_state (
    game_id INTEGER NOT NULL,
    count_seq INTEGER NOT NULL,
    state_version INTEGER NOT NULL,
    state_source TEXT NOT NULL,
    invalid_reason TEXT,
    state_json TEXT,
    PRIMARY KEY (game_id, count_seq),
    CHECK ((invalid_reason IS NULL AND state_json IS NOT NULL)
        OR (invalid_reason IS NOT NULL AND state_json IS NULL)),
    CHECK (state_json IS NULL OR json_valid(state_json))
);
