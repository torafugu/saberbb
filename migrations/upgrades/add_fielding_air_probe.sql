-- Apply once after add_fielding_candidate_diagnostics.sql.
-- Historical candidate rows remain NULL; new simulations populate the probe.
BEGIN TRANSACTION;
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_step_sec REAL CHECK (air_probe_step_sec > 0);
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_air_samples INTEGER CHECK (air_probe_air_samples >= 0);
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_height_ok_samples INTEGER CHECK (air_probe_height_ok_samples >= 0);
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_reachable_samples INTEGER CHECK (air_probe_reachable_samples >= 0);
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_risk_ok_samples INTEGER CHECK (air_probe_risk_ok_samples >= 0);
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_max_margin_sec REAL;
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_max_margin_time_sec REAL;
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_max_margin_height_m REAL;
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_first_catch_time_sec REAL;
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_first_catch_height_m REAL;
ALTER TABLE player_game_fielding_candidate ADD COLUMN air_probe_first_catch_margin_sec REAL;
COMMIT;
