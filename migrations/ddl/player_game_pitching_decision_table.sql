DROP TABLE player_game_pitching_decision;

CREATE TABLE player_game_pitching_decision (
    game_id INTEGER NOT NULL,
    pitcher_id INTEGER NOT NULL,
    decision TEXT NOT NULL,
    PRIMARY KEY (game_id, pitcher_id, decision)
);
