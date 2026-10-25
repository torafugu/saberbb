SELECT
    result,
    COUNT(*) AS n,
    ROUND(
        100.0 * COUNT(*) / SUM(COUNT(*)) OVER (),
        2
    ) AS pct
FROM player_game_batting
GROUP BY result
ORDER BY n DESC;
