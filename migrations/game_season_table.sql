DROP TABLE game_season;

CREATE TABLE game_season (
    start_season INTEGER NOT NULL,
    start_date TEXT NOT NULL,
    current_season INTEGER NOT NULL,
    current_round_seq INTEGER NOT NULL,
    scheduled_season INTEGER NOT NULL
);