-- Apply once after add_fielding_air_probe.sql on existing databases.
BEGIN;
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_coarse_step_sec REAL CHECK (air_probe_coarse_step_sec > 0);
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_coarse_air_samples INTEGER CHECK (air_probe_coarse_air_samples >= 0);
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_coarse_height_ok_samples INTEGER CHECK (air_probe_coarse_height_ok_samples >= 0);
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_coarse_reachable_samples INTEGER CHECK (air_probe_coarse_reachable_samples >= 0);
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_coarse_risk_ok_samples INTEGER CHECK (air_probe_coarse_risk_ok_samples >= 0);
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_coarse_max_margin_sec REAL;
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_coarse_max_margin_time_sec REAL;
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_coarse_max_margin_height_m REAL;
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_coarse_first_catch_time_sec REAL;
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_coarse_first_catch_height_m REAL;
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_coarse_first_catch_margin_sec REAL;
COMMIT;
