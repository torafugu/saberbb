SELECT
    result,
    COUNT(*) AS n,
    ROUND(AVG(launch_speed * 3.6), 1) AS avg_exit_kmh,
    ROUND(AVG(launch_angle), 1) AS avg_launch_angle,
    ROUND(AVG(polar_distance), 1) AS avg_distance_m,
    ROUND(AVG(total_time), 2) AS avg_total_time_sec
FROM player_game_batting
GROUP BY result
ORDER BY n DESC;
