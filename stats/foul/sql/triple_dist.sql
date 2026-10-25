SELECT
    fielder_position,
    COUNT(*) AS triples,
    ROUND(AVG(polar_distance), 1) AS avg_distance_m,
    ROUND(AVG(total_time), 2) AS avg_ball_time,
    ROUND(AVG(ABS(polar_angle)), 1) AS avg_abs_angle
FROM player_game_batting
WHERE result = 'Triple'
GROUP BY fielder_position
ORDER BY triples DESC;
