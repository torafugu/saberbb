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
    total_time REAL NOT NULL,
    first_bounce_distance REAL,
    first_bounce_angle REAL,
    first_bounce_time REAL,
    fence_impact_distance REAL,
    fence_impact_angle REAL,
    fence_impact_time REAL,
    outbound_result TEXT NOT NULL,
    fielder_position TEXT,
    result TEXT NOT NULL,
    PRIMARY KEY (game_id, count_seq)
);