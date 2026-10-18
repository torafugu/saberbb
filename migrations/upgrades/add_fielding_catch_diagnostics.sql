-- Apply once to an existing database. Existing rows remain NULL.
BEGIN TRANSACTION;
ALTER TABLE player_game_fielding ADD COLUMN selection_path TEXT;
ALTER TABLE player_game_fielding ADD COLUMN interception_fielder_position TEXT;
ALTER TABLE player_game_fielding ADD COLUMN catch_type TEXT;
ALTER TABLE player_game_fielding ADD COLUMN error_type TEXT;
ALTER TABLE player_game_fielding ADD COLUMN catch_time_sec REAL;
ALTER TABLE player_game_fielding ADD COLUMN catch_z_m REAL;
ALTER TABLE player_game_fielding ADD COLUMN waiting_time_sec REAL;
ALTER TABLE player_game_fielding ADD COLUMN first_bounce_time REAL;
COMMIT;
