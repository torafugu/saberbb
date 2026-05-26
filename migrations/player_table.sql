DROP TABLE player;

CREATE TABLE player (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id INTEGER NOT NULL,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    age INTEGER NOT NULL,
    throw TEXT NOT NULL,
    bat TEXT NOT NULL,
    mod_ba REAL NOT NULL,
    mod_slg REAL NOT NULL
);