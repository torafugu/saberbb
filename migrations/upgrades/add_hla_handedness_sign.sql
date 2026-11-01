-- Apply once; historical HLA orientation stays NULL.
BEGIN;
ALTER TABLE player_game_batting ADD COLUMN hla_handedness_sign REAL CHECK (hla_handedness_sign IN (-1,1));
COMMIT;
