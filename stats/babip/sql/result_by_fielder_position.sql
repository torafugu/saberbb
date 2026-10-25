SELECT
    fielder_position,
    result,
    COUNT(*) AS n
FROM
    player_game_batting
WHERE
    fielder_position IS NOT NULL
GROUP BY
    fielder_position,
    result
ORDER BY
    fielder_position,
    n DESC;