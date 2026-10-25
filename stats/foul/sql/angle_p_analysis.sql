WITH batted AS (
    SELECT
        ABS(timing_error) * 1000.0 AS abs_timing_error,
        ABS(timing_angle_error) AS abs_timing_angle_deg
    FROM
        player_game_batting
    WHERE
        result IN (
            'Out',
            'Single',
            'Double',
            'Triple',
            'HomeRun',
            'DoublePlay',
            'FieldersChoice',
            'Foul'
        )
),
timing_ranked AS (
    SELECT
        abs_timing_error AS value,
        ROW_NUMBER() OVER (
            ORDER BY
                abs_timing_error
        ) AS rn,
        COUNT(*) OVER () AS n
    FROM
        batted
),
angle_ranked AS (
    SELECT
        abs_timing_angle_deg AS value,
        ROW_NUMBER() OVER (
            ORDER BY
                abs_timing_angle_deg
        ) AS rn,
        COUNT(*) OVER () AS n
    FROM
        batted
)
SELECT
    (
        SELECT
            value
        FROM
            timing_ranked
        WHERE
            rn = CAST((n - 1) * 0.50 AS INTEGER) + 1
    ) AS timing_p50_ms,
    (
        SELECT
            value
        FROM
            timing_ranked
        WHERE
            rn = CAST((n - 1) * 0.90 AS INTEGER) + 1
    ) AS timing_p90_ms,
    (
        SELECT
            value
        FROM
            timing_ranked
        WHERE
            rn = CAST((n - 1) * 0.95 AS INTEGER) + 1
    ) AS timing_p95_ms,
    (
        SELECT
            value
        FROM
            timing_ranked
        WHERE
            rn = CAST((n - 1) * 0.99 AS INTEGER) + 1
    ) AS timing_p99_ms,
    (
        SELECT
            value
        FROM
            angle_ranked
        WHERE
            rn = CAST((n - 1) * 0.50 AS INTEGER) + 1
    ) AS angle_p50_deg,
    (
        SELECT
            value
        FROM
            angle_ranked
        WHERE
            rn = CAST((n - 1) * 0.90 AS INTEGER) + 1
    ) AS angle_p90_deg,
    (
        SELECT
            value
        FROM
            angle_ranked
        WHERE
            rn = CAST((n - 1) * 0.95 AS INTEGER) + 1
    ) AS angle_p95_deg,
    (
        SELECT
            value
        FROM
            angle_ranked
        WHERE
            rn = CAST((n - 1) * 0.99 AS INTEGER) + 1
    ) AS angle_p99_deg;