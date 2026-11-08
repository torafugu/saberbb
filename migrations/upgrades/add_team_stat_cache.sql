-- Derived team counts, attributed by the game's batting/fielding side, not current roster.
CREATE TABLE IF NOT EXISTS game_team_stat_cache (
    game_id INTEGER NOT NULL, team_id INTEGER NOT NULL,
    games INTEGER NOT NULL, wins INTEGER NOT NULL, losses INTEGER NOT NULL, draws INTEGER NOT NULL,
    runs INTEGER NOT NULL, runs_allowed INTEGER NOT NULL, homeruns INTEGER NOT NULL,
    steals INTEGER NOT NULL, hits INTEGER NOT NULL, at_bats INTEGER NOT NULL,
    pitching_outs INTEGER NOT NULL, errors INTEGER NOT NULL,
    PRIMARY KEY(game_id,team_id)
);
CREATE INDEX IF NOT EXISTS team_stat_cache_team ON game_team_stat_cache(team_id);
CREATE TABLE IF NOT EXISTS team_stat_totals (
    team_id INTEGER PRIMARY KEY,
    games INTEGER NOT NULL, wins INTEGER NOT NULL, losses INTEGER NOT NULL, draws INTEGER NOT NULL,
    runs INTEGER NOT NULL, runs_allowed INTEGER NOT NULL, homeruns INTEGER NOT NULL,
    steals INTEGER NOT NULL, hits INTEGER NOT NULL, at_bats INTEGER NOT NULL,
    pitching_outs INTEGER NOT NULL, errors INTEGER NOT NULL
);
