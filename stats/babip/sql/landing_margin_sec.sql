WITH targets AS (
    SELECT
        c.game_id,
        c.count_seq,
        c.fielder_id,
        c.fielder_position,
        c.selected_path,
        c.required_margin_sec,
        b.launch_speed,
        b.launch_angle,
        b.first_bounce_time,
        b.first_bounce_distance,
        b.first_bounce_angle,
        b.polar_distance AS final_distance,
        f.running_speed,
        f.prep_time,
        f.reach_height
    FROM
        player_game_fielding_candidate AS c
        JOIN player_game_batting AS b ON b.game_id = c.game_id
        AND b.count_seq = c.count_seq
        LEFT JOIN fielder_info AS f ON f.player_id = c.fielder_id
        AND f.fielder_type = 'Outfielder'
    WHERE
        c.fielder_position = 'CF'
        AND c.end_reason = 'filtered_distance'
        AND c.selected = 1 -- 前回の集計と同じ打球条件
        AND b.result = 'Single'
        AND b.fielder_position IN ('LF', 'CF', 'RF')
        AND b.launch_speed >= 25
        AND b.launch_speed < 35
        AND b.launch_angle >= 25
        AND b.first_bounce_time IS NOT NULL
        AND b.fence_impact_time IS NULL
),
distances AS (
    SELECT
        *,
        -- CF初期位置：距離90m・角度0°
        -- 余弦定理で、初期位置から着地点までの移動距離を計算
        SQRT(
            MAX(
                0.0,
                90.0 * 90.0 + first_bounce_distance * first_bounce_distance - 2.0 * 90.0 * first_bounce_distance * COS(first_bounce_angle * 0.017453292519943295)
            )
        ) AS movement_to_landing_m
    FROM
        targets
),
arrival AS (
    SELECT
        *,
        CASE
            WHEN running_speed > 0 THEN prep_time + movement_to_landing_m / running_speed
        END AS needed_time_sec
    FROM
        distances
)
SELECT
    game_id,
    count_seq,
    fielder_id,
    fielder_position,
    selected_path,
    ROUND(final_distance, 3) AS final_distance,
    ROUND(first_bounce_distance, 3) AS landing_distance,
    ROUND(first_bounce_angle, 3) AS landing_angle,
    ROUND(first_bounce_time, 3) AS landing_time_sec,
    running_speed,
    prep_time,
    reach_height,
    ROUND(movement_to_landing_m, 3) AS movement_to_landing_m,
    ROUND(needed_time_sec, 3) AS needed_time_sec,
    ROUND(first_bounce_time - needed_time_sec, 3) AS landing_margin_sec,
    required_margin_sec
FROM
    arrival
ORDER BY
    game_id,
    count_seq;