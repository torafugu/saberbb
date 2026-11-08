-- Targeted repair for diagnostic rows left behind by an old database setup.
-- Run with the app and other writers closed, against an initialized database.
-- Preserve diagnostics for completed games; never recreate/reset the database.
BEGIN IMMEDIATE;

DELETE FROM player_game_fielding_candidate
WHERE NOT EXISTS (
    SELECT 1 FROM game g
    WHERE g.id = player_game_fielding_candidate.game_id
      AND g.actual_date IS NOT NULL
);

DELETE FROM player_game_contact_state
WHERE NOT EXISTS (
    SELECT 1 FROM game g
    WHERE g.id = player_game_contact_state.game_id
      AND g.actual_date IS NOT NULL
);

COMMIT;
