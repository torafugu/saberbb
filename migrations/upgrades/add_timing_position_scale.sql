-- Apply once. NULL on historical swings denotes the legacy unscaled policy.
BEGIN;
ALTER TABLE player_game_batting ADD COLUMN timing_position_scale REAL CHECK (timing_position_scale > 0);
COMMIT;
