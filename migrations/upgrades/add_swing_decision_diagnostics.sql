-- Apply once to an existing database. Legacy rows stay NULL.
BEGIN;
ALTER TABLE player_game_batting ADD COLUMN decision_count_status TEXT;
ALTER TABLE player_game_batting ADD COLUMN decision_location_x REAL;
ALTER TABLE player_game_batting ADD COLUMN decision_location_y REAL;
ALTER TABLE player_game_batting ADD COLUMN decision_count_factor REAL;
ALTER TABLE player_game_batting ADD COLUMN decision_approach_factor REAL;
ALTER TABLE player_game_batting ADD COLUMN decision_protection_factor REAL;
ALTER TABLE player_game_batting ADD COLUMN decision_zone_factor REAL;
ALTER TABLE player_game_batting ADD COLUMN decision_pitch_factor REAL;
ALTER TABLE player_game_batting ADD COLUMN decision_anticipation_factor REAL;
ALTER TABLE player_game_batting ADD COLUMN decision_aptitude_factor REAL;
ALTER TABLE player_game_batting ADD COLUMN decision_score REAL;
ALTER TABLE player_game_batting ADD COLUMN decision_probability REAL;
ALTER TABLE player_game_batting ADD COLUMN decision_swung INTEGER;
COMMIT;
