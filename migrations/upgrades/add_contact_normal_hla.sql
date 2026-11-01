-- Apply once. Legacy HLA version NULL denotes version 1.
BEGIN;
ALTER TABLE player_game_batting ADD COLUMN signed_length_offset_m REAL;
ALTER TABLE player_game_batting ADD COLUMN signed_thickness_offset_m REAL;
ALTER TABLE player_game_batting ADD COLUMN hla_model_version INTEGER CHECK (hla_model_version IN (1,2));
COMMIT;
