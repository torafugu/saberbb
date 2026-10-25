SELECT
    outbound_result,
    result,
    COUNT(*) AS n
FROM player_game_batting
GROUP BY outbound_result, result
ORDER BY outbound_result, n DESC;
