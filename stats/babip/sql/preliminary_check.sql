WITH initial_positions(
    fielder_position,
    initial_distance,
    initial_angle
) AS (
    VALUES
        ('CF', 90.0, 0.0),
        ('LF', 80.0, -26.0),
        ('RF', 80.0, 26.0)
),
targets AS (
    SELECT
        c.game_id,
        c.count_seq,
        c.fielder_id,
        c.fielder_position,
        c.distance_eligible,
        c.selected_path,
        c.required_margin_sec,
        p.initial_distance,
        p.initial_angle,
        b.launch_speed,
        b.launch_angle,
        b.first_bounce_time,
        b.first_bounce_distance,
        b.first_bounce_angle,
        b.polar_distance AS final_distance,
        b.polar_angle AS final_angle,
        f.running_speed,
        f.prep_time,
        f.reach_height
    FROM
        player_game_fielding_candidate AS c
        JOIN player_game_batting AS b ON b.game_id = c.game_id
        AND b.count_seq = c.count_seq
        JOIN initial_positions AS p ON p.fielder_position = c.fielder_position
        LEFT JOIN fielder_info AS f ON f.player_id = c.fielder_id
        AND f.fielder_type = 'Outfielder'
    WHERE
        c.end_reason = 'filtered_lane'
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
        SQRT(
            MAX(
                0.0,
                initial_distance * initial_distance + first_bounce_distance * first_bounce_distance - 2.0 * initial_distance * first_bounce_distance * COS(
                    (first_bounce_angle - initial_angle) * 0.017453292519943295
                )
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
    distance_eligible,
    selected_path,
    initial_distance,
    initial_angle,
    ROUND(final_angle, 3) AS final_angle,
    ROUND(first_bounce_angle, 3) AS landing_angle,
    ROUND(first_bounce_distance, 3) AS landing_distance,
    ROUND(first_bounce_time, 3) AS landing_time_sec,
    running_speed,
    prep_time,
    reach_height,
    ROUND(movement_to_landing_m, 3) AS movement_to_landing_m,
    ROUND(needed_time_sec, 3) AS needed_time_sec,
    ROUND(first_bounce_time - needed_time_sec, 3) AS landing_margin_sec,
    required_margin_sec,
    CASE
        WHEN needed_time_sec IS NULL THEN 'missing_ability'
        WHEN first_bounce_time - needed_time_sec >= required_margin_sec THEN 'air_search_needed'
        WHEN first_bounce_time - needed_time_sec >= 0 THEN 'small_margin_at_landing'
        ELSE 'late_at_landing'
    END AS preliminary_check
FROM
    arrival
ORDER BY
    landing_margin_sec DESC;