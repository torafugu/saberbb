DROP TABLE running_skills;

CREATE TABLE running_skills (
    player_id INTEGER PRIMARY KEY,
    speed REAL NOT NULL,
    lead_distance REAL NOT NULL,
    start_reaction REAL NOT NULL
);