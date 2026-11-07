-- Additive, derived data only. First use backfills existing pitch logs once.
CREATE TABLE IF NOT EXISTS stat_cache_state (
    id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL, ready INTEGER NOT NULL
);
INSERT OR IGNORE INTO stat_cache_state VALUES(1,2,0);
CREATE TABLE IF NOT EXISTS game_batting_stat_cache (
    game_id INTEGER NOT NULL, player_id INTEGER NOT NULL,
    ab INTEGER NOT NULL, single INTEGER NOT NULL, double INTEGER NOT NULL,
    triple INTEGER NOT NULL, homerun INTEGER NOT NULL, rbi INTEGER NOT NULL,
    PRIMARY KEY(game_id,player_id)
);
CREATE INDEX IF NOT EXISTS batting_stat_cache_player ON game_batting_stat_cache(player_id);
CREATE TABLE IF NOT EXISTS game_pitching_stat_cache (
    game_id INTEGER NOT NULL, player_id INTEGER NOT NULL,
    games INTEGER NOT NULL, innings INTEGER NOT NULL, runs INTEGER NOT NULL,
    wins INTEGER NOT NULL, losses INTEGER NOT NULL, saves INTEGER NOT NULL, holds INTEGER NOT NULL,
    so INTEGER NOT NULL, bb INTEGER NOT NULL,
    PRIMARY KEY(game_id,player_id)
);
CREATE INDEX IF NOT EXISTS pitching_stat_cache_player ON game_pitching_stat_cache(player_id);

CREATE TABLE IF NOT EXISTS batting_stat_totals (
    player_id INTEGER PRIMARY KEY, ab INTEGER NOT NULL, single INTEGER NOT NULL,
    double INTEGER NOT NULL, triple INTEGER NOT NULL, homerun INTEGER NOT NULL, rbi INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS pitching_stat_totals (
    player_id INTEGER PRIMARY KEY, games INTEGER NOT NULL, innings INTEGER NOT NULL, runs INTEGER NOT NULL,
    wins INTEGER NOT NULL, losses INTEGER NOT NULL, saves INTEGER NOT NULL, holds INTEGER NOT NULL,
    so INTEGER NOT NULL, bb INTEGER NOT NULL
);
