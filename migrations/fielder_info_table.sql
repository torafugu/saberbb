DROP TABLE fielder_info;

CREATE TABLE fielder_info (
    player_id INTEGER,
    fielder_type TEXT,
    throw_speed REAL NOT NULL,
    running_speed REAL NOT NULL,
    reaction REAL NOT NULL,
    prep_time REAL NOT NULL,
    catching REAL NOT NULL,
    PRIMARY KEY (player_id, fielder_type)
);