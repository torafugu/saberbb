DROP TABLE IF EXISTS player_batting_character;
CREATE TABLE player_batting_character (player_id INTEGER PRIMARY KEY REFERENCES player_info(id),character_json TEXT NOT NULL);
