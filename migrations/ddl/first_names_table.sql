DROP TABLE IF EXISTS first_names;

CREATE TABLE first_names (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT,
    reading TEXT,
    gender TEXT,
    country TEXT
);
