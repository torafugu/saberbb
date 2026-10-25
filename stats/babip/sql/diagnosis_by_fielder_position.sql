WITH classified AS (
    SELECT
        c.fielder_position,
        c.selected,
        CASE
            WHEN c.evaluated = 0 THEN c.end_reason
            WHEN c.air_samples = 0 THEN 'no_air_samples'
            WHEN c.height_ok_samples = 0 THEN 'no_catchable_height'
            WHEN c.reachable_samples = 0 THEN 'too_late'
            WHEN c.risk_ok_samples = 0 THEN 'risk_rejected'
            WHEN c.interception_error_type IS NOT NULL THEN 'fielding_error'
            ELSE 'air_conditions_passed'
        END AS diagnosis
    FROM
        player_game_fielding_candidate AS c
        JOIN player_game_batting AS b ON b.game_id = c.game_id
        AND b.count_seq = c.count_seq
    WHERE
        b.result = 'Single'
        AND b.fielder_position IN ('LF', 'CF', 'RF')
        AND b.launch_speed >= 25
        AND b.launch_speed < 35
        AND b.launch_angle >= 25
        AND b.first_bounce_time IS NOT NULL
        AND b.fence_impact_time IS NULL
        AND c.fielder_position IN ('LF', 'CF', 'RF')
)
SELECT
    fielder_position,
    diagnosis,
    selected,
    COUNT(*) AS n
FROM
    classified
GROUP BY
    fielder_position,
    diagnosis,
    selected
ORDER BY
    fielder_position,
    n DESC;