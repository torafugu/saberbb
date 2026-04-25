DROP TABLE game;

CREATE TABLE game (
    game_round_season INTEGER,
    game_round_seq INTEGER,
    seq INTEGER,
    date TEXT NOT NULL,
    away_team_id INTEGER NOT NULL,
    home_team_id INTEGER NOT NULL,
    game_type TEXT NOT NULL,
    PRIMARY KEY (
        game_round_season,
        game_round_seq,
        seq,
        away_team_id,
        home_team_id
    )
);