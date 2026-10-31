-- Apply once; historical angles remain unknown.
BEGIN;
ALTER TABLE player_game_batting ADD COLUMN actual_bat_angle_deg REAL;
COMMIT;
