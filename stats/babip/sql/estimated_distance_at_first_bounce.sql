SELECT
    game_id,
    count_seq,
    launch_speed,
    launch_angle,
    total_time,
    first_bounce_time,
    ROUND(total_time - first_bounce_time, 3) AS time_after_first_bounce_sec,
    first_bounce_distance,
    polar_distance AS final_distance,
    -- 現行の通常打球の補間式による、着地時刻の推定距離
    ROUND(
        polar_distance * first_bounce_time / NULLIF(total_time, 0),
        3
    ) AS estimated_distance_at_first_bounce,
    first_bounce_angle,
    polar_angle AS final_angle
FROM
    player_game_batting
WHERE
    game_id = 16075
    AND count_seq = 185;