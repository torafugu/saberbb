DROP TABLE league;

CREATE TABLE league (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    number_of_games INTEGER,
    max_inning INTEGER,
    base_four_seam_speed REAL
);
