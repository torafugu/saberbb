DROP TABLE game;

CREATE TABLE game (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    season INTEGER,
    round_seq INTEGER,
    seq INTEGER,
    planned_date TEXT NOT NULL,
    actual_date TEXT,
    away_team_id INTEGER NOT NULL,
    home_team_id INTEGER NOT NULL,
    game_type TEXT NOT NULL,
    away_points INTEGER,
    home_points INTEGER
);