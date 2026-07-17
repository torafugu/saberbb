DROP TABLE stadium;

CREATE TABLE stadium (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    foul_pole_distance REAL NOT NULL,
    center_fence_distance REAL NOT NULL,
    fence_line TEXT,
    fence_height REAL NOT NULL
);
