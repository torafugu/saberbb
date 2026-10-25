SELECT
    game_id,
    count_seq,
    COUNT(*) AS fielder_rows,
    SUM(evaluated) AS evaluated_fielders,
    SUM(selected) AS selected_fielders
FROM
    player_game_fielding_candidate
GROUP BY
    game_id,
    count_seq
ORDER BY
    game_id DESC,
    count_seq
LIMIT
    50;