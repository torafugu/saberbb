SELECT
    f.interception_fielder_position AS fielder_position,
    f.selection_path,
    f.catch_type,
    f.error_type,
    b.result,
    COUNT(*) AS n,
    ROUND(AVG(f.waiting_time_sec), 3) AS avg_waiting_time_sec
FROM
    player_game_fielding AS f
    JOIN player_game_batting AS b ON b.game_id = f.game_id
    AND b.count_seq = f.count_seq
WHERE
    f.seq = 1
    AND f.selection_path IS NOT NULL
    AND f.interception_fielder_position IN ('LF', 'CF', 'RF')
GROUP BY
    f.interception_fielder_position,
    f.selection_path,
    f.catch_type,
    f.error_type,
    b.result
ORDER BY
    fielder_position,
    n DESC;