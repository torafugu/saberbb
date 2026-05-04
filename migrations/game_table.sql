DROP TABLE game;

CREATE TABLE game (
    game_round_id INTEGER,
    id INTEGER,
    planned_date TEXT NOT NULL,
    actual_date TEXT NOT NULL,
    away_team_id INTEGER NOT NULL,
    home_team_id INTEGER NOT NULL,
    game_type TEXT NOT NULL,
    away_point INTEGER NOT NULL,
    home_point INTEGER NOT NULL,
    PRIMARY KEY (id)
);