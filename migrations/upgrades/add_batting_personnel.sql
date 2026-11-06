-- Idempotent, non-destructive. Existing people and assignments are retained.
BEGIN TRANSACTION;
CREATE TABLE IF NOT EXISTS player_batting_character (
    player_id INTEGER PRIMARY KEY REFERENCES player_info(id),
    character_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS manager (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    batting_policy TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS team_manager (
    team_id INTEGER PRIMARY KEY REFERENCES team(id),
    manager_id INTEGER NOT NULL UNIQUE REFERENCES manager(id)
);
COMMIT;
