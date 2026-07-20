DROP TABLE player_game_batting;

CREATE TABLE player_game_batting (
    game_id INTEGER,
    count_seq INTEGER,
    pitcher_id INTEGER NOT NULL,
    batter_id INTEGER NOT NULL,
    launch_speed REAL NOT NULL,
    launch_angle REAL NOT NULL,
    polar_distance REAL NOT NULL,
    polar_angle REAL NOT NULL,
    hang_time REAL NOT NULL,
    trajectory TEXT NOT NULL,
    fielder_position TEXT,
    result TEXT NOT NULL,
    PRIMARY KEY (game_id, count_seq)
);