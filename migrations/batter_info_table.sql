DROP TABLE batter_info;

CREATE TABLE batter_info (
    player_id INTEGER PRIMARY KEY,
    batting_side TEXT NOT NULL,
    weight_pull REAL NOT NULL,
    weight_center REAL NOT NULL,
    weight_opposite REAL NOT NULL,
    weight_foul_left REAL NOT NULL,
    weight_foul_right REAL NOT NULL
);