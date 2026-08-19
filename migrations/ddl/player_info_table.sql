DROP TABLE player_info;

CREATE TABLE player_info (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id INTEGER NOT NULL,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    age INTEGER NOT NULL,
    uniform_number INTEGER NOT NULL
);