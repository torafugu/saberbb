DROP TABLE batting_record;

CREATE TABLE player_game_batting (
    game_id INTEGER,
    count_seq INTEGER,
    team_id INTEGER,
    pitcher_id INTEGER,
    batter_id INTEGER,
    result TEXT NOT NULL,
    PRIMARY KEY (
        game_id,
        count_seq,
        batter_id
    )
);