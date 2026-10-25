SELECT
    CASE
        WHEN launch_angle < -10 THEN '<-10'
        WHEN launch_angle < 0 THEN '-10..0'
        WHEN launch_angle < 10 THEN '0..10'
        WHEN launch_angle < 25 THEN '10..25'
        WHEN launch_angle < 40 THEN '25..40'
        ELSE '40+'
    END AS la_band,
    result,
    COUNT(*) AS n
FROM
    player_game_batting
WHERE
    launch_speed > 40
GROUP BY
    la_band,
    result
ORDER BY
    la_band;