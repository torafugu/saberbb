SELECT
    CASE
        WHEN launch_speed < 25 THEN '<25'
        WHEN launch_speed < 35 THEN '25-35'
        WHEN launch_speed < 45 THEN '35-45'
        WHEN launch_speed < 50 THEN '45-50'
        ELSE '50+'
    END AS ev_band,
    result,
    COUNT(*) AS n
FROM
    player_game_batting
WHERE
    launch_speed > 0
GROUP BY
    ev_band,
    result
ORDER BY
    ev_band;