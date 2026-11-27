DROP TABLE IF EXISTS catcher_info;

CREATE TABLE catcher_info (
    player_id INTEGER PRIMARY KEY,
    calling_style TEXT NOT NULL,
    pitch_blocking REAL NOT NULL DEFAULT 0.8 CHECK (pitch_blocking BETWEEN 0 AND 1.6)
);
