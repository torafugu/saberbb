DROP TABLE batter;

CREATE TABLE batter (
    id INTEGER PRIMARY KEY,
    team_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    mod_ba REAL NOT NULL,
    mod_slg REAL NOT NULL
);