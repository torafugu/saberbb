DROP TABLE IF EXISTS team_manager;
CREATE TABLE team_manager (team_id INTEGER PRIMARY KEY REFERENCES team(id),manager_id INTEGER NOT NULL UNIQUE REFERENCES manager(id));
