WITH RECURSIVE target AS (
    SELECT
        b.first_bounce_time,
        b.first_bounce_distance,
        b.first_bounce_angle,
        f.running_speed,
        f.prep_time,
        f.reach_height,
        c.required_margin_sec
    FROM
        player_game_batting AS b
        JOIN player_game_fielding_candidate AS c ON c.game_id = b.game_id
        AND c.count_seq = b.count_seq
        JOIN fielder_info AS f ON f.player_id = c.fielder_id
        AND f.fielder_type = 'Outfielder'
    WHERE
        b.game_id = 16712
        AND b.count_seq = 22
        AND c.fielder_position = 'LF'
        AND b.fence_impact_time IS NULL
        AND b.first_bounce_time > 0
        AND f.running_speed > 0
),
steps(step_sec) AS (
    VALUES
        (0.05),
        (0.01)
),
ticks(step_sec, tick) AS (
    SELECT
        step_sec,
        0
    FROM
        steps
        CROSS JOIN target
    WHERE
        0.10 < first_bounce_time
    UNION
    ALL
    SELECT
        step_sec,
        tick + 1
    FROM
        ticks
        CROSS JOIN target
    WHERE
        0.10 + (tick + 1) * step_sec < first_bounce_time
),
points AS (
    SELECT
        step_sec,
        0.10 + tick * step_sec AS t,
        target.*
    FROM
        ticks
        CROSS JOIN target
),
distances AS (
    SELECT
        *,
        SQRT(
            MAX(
                0.0,
                80.0 * 80.0 + (first_bounce_distance * t / first_bounce_time) * (first_bounce_distance * t / first_bounce_time) - 2.0 * 80.0 * (first_bounce_distance * t / first_bounce_time) * COS(
                    (first_bounce_angle - (-26.0)) * 0.017453292519943295
                )
            )
        ) AS movement_m
    FROM
        points
),
margins AS (
    SELECT
        *,
        prep_time + movement_m / running_speed AS needed_time_sec,
        t - prep_time - movement_m / running_speed AS margin_sec,
        -- この時刻で高さ条件を満たすmax_heightの上限
        reach_height / NULLIF(
            4.0 * (t / first_bounce_time) * (1.0 - t / first_bounce_time),
            0.0
        ) AS max_height_allowed_m
    FROM
        distances
),
ranked AS (
    SELECT
        *,
        ROW_NUMBER() OVER (
            PARTITION BY step_sec
            ORDER BY
                t DESC
        ) AS reverse_seq
    FROM
        margins
)
SELECT
    step_sec,
    ROUND(t, 3) AS search_time_sec,
    ROUND(movement_m, 3) AS movement_m,
    ROUND(needed_time_sec, 3) AS needed_time_sec,
    ROUND(margin_sec, 3) AS margin_sec,
    required_margin_sec,
    margin_sec >= required_margin_sec AS time_condition_ok,
    ROUND(max_height_allowed_m, 3) AS max_height_allowed_m
FROM
    ranked -- 要求余裕を満たす点と、各刻みの最後の探索点を表示
WHERE
    margin_sec >= required_margin_sec
    OR reverse_seq = 1
ORDER BY
    step_sec DESC,
    t;