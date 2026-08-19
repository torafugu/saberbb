DROP TABLE game_season;

CREATE TABLE game_season (
    season_start_date TEXT NOT NULL,
    scheduled_season INTEGER NOT NULL,
    current_season INTEGER NOT NULL,
    current_round_seq INTEGER NOT NULL
);