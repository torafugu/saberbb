-- Run once on an existing database created before catcher_info was added.
BEGIN TRANSACTION;
CREATE TABLE catcher_info (
    player_id INTEGER PRIMARY KEY,
    calling_style TEXT NOT NULL
);
INSERT INTO catcher_info (player_id, calling_style)
SELECT player_id, 'Balanced' FROM fielder_info WHERE fielder_type = 'Catcher';
COMMIT;
