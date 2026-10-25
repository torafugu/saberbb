WITH batted AS (
    SELECT
        timing_error * 1000.0 AS timing_error_actual_ms,
        timing_angle_error,
        base_hla_deg,
        final_hla_deg,
        result
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
banded AS (
    SELECT
        CASE
            WHEN ABS(timing_error_actual_ms) < 5.0 THEN '00-05ms'
            WHEN ABS(timing_error_actual_ms) < 10.0 THEN '05-10ms'
            WHEN ABS(timing_error_actual_ms) < 15.0 THEN '10-15ms'
            WHEN ABS(timing_error_actual_ms) < 20.0 THEN '15-20ms'
            WHEN ABS(timing_error_actual_ms) < 25.0 THEN '20-25ms'
            WHEN ABS(timing_error_actual_ms) < 30.0 THEN '25-30ms'
            WHEN ABS(timing_error_actual_ms) < 40.0 THEN '30-40ms'
            ELSE '40ms+'
        END AS timing_band,
        CASE
            WHEN ABS(timing_error_actual_ms) < 5.0 THEN 1
            WHEN ABS(timing_error_actual_ms) < 10.0 THEN 2
            WHEN ABS(timing_error_actual_ms) < 15.0 THEN 3
            WHEN ABS(timing_error_actual_ms) < 20.0 THEN 4
            WHEN ABS(timing_error_actual_ms) < 25.0 THEN 5
            WHEN ABS(timing_error_actual_ms) < 30.0 THEN 6
            WHEN ABS(timing_error_actual_ms) < 40.0 THEN 7
            ELSE 8
        END AS sort_key,
        timing_angle_error,
        base_hla_deg,
        final_hla_deg,
        result
    FROM
        batted
)
SELECT
    timing_band,
    COUNT(*) AS n,
    ROUND(
        AVG(ABS(timing_angle_error)),
        2
    ) AS avg_timing_angle_deg,
    ROUND(AVG(ABS(base_hla_deg)), 2) AS avg_abs_base_hla_deg,
    ROUND(AVG(ABS(final_hla_deg)), 2) AS avg_abs_final_hla_deg,
    SUM(
        ABS(base_hla_deg) <= 45.0
        AND ABS(final_hla_deg) > 45.0
    ) AS crossed_into_foul,
    SUM(
        ABS(base_hla_deg) > 45.0
        AND ABS(final_hla_deg) <= 45.0
    ) AS crossed_into_fair,
    SUM(result = 'Foul') AS foul_results
FROM
    banded
GROUP BY
    timing_band,
    sort_key
ORDER BY
    sort_key;