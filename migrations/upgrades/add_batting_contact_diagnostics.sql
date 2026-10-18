-- Run once on an existing database. Historical diagnostics remain NULL.
BEGIN TRANSACTION;
ALTER TABLE player_game_batting ADD COLUMN adapted_horizontal_offset_m REAL;
ALTER TABLE player_game_batting ADD COLUMN adapted_vertical_offset_m REAL;
ALTER TABLE player_game_batting ADD COLUMN adapted_timing_offset_sec REAL;
ALTER TABLE player_game_batting ADD COLUMN timing_impact_x_m REAL;
ALTER TABLE player_game_batting ADD COLUMN swing_error_x_m REAL;
ALTER TABLE player_game_batting ADD COLUMN swing_error_z_m REAL;
ALTER TABLE player_game_batting ADD COLUMN contact_offset_x_m REAL;
ALTER TABLE player_game_batting ADD COLUMN contact_offset_z_m REAL;
ALTER TABLE player_game_batting ADD COLUMN thickness_offset_m REAL;
COMMIT;
