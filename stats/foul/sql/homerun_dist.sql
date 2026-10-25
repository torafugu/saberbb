SELECT
    ROUND(launch_speed * 3.6 / 10.0) * 10 AS exit_kmh_bin,
    CAST(launch_angle / 5 AS INTEGER) * 5 AS angle_bin,
    COUNT(*) AS n,
    ROUND(AVG(polar_distance), 1) AS avg_distance_m
FROM player_game_batting
WHERE result = 'HomeRun'
GROUP BY exit_kmh_bin, angle_bin
ORDER BY exit_kmh_bin, angle_bin;
