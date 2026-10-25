WITH batted AS (
    SELECT
        timing_error,
        bat_speed,
        timing_angle_error,
        -- 列の実体が「秒」だった場合
        ABS(
            bat_speed / 1.10 * timing_error * 57.29577951308232
        ) AS expected_if_seconds,
        -- 列の実体が本当に「ミリ秒」だった場合
        ABS(
            bat_speed / 1.10 * timing_error / 1000.0 * 57.29577951308232
        ) AS expected_if_milliseconds
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
        AND timing_error IS NOT NULL
        AND bat_speed IS NOT NULL
        AND timing_angle_error IS NOT NULL
)
SELECT
    COUNT(*) AS n,
    ROUND(
        AVG(ABS(timing_angle_error)),
        4
    ) AS avg_actual_angle_deg,
    ROUND(
        AVG(expected_if_seconds),
        4
    ) AS avg_expected_if_seconds,
    ROUND(
        AVG(expected_if_milliseconds),
        6
    ) AS avg_expected_if_ms,
    ROUND(
        AVG(
            ABS(
                ABS(timing_angle_error) - expected_if_seconds
            )
        ),
        6
    ) AS error_if_seconds,
    ROUND(
        AVG(
            ABS(
                ABS(timing_angle_error) - expected_if_milliseconds
            )
        ),
        6
    ) AS error_if_ms
FROM
    batted;