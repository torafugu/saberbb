SELECT
    MIN(game_id) AS first_game_id,
    MAX(game_id) AS last_game_id
FROM
    player_game_batting;