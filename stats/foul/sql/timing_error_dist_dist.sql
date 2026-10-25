WITH contact AS (
    SELECT
        *
    FROM
        player_game_batting
    WHERE
        timing_error IS NOT NULL
),
banded AS (
    SELECT
        CASE
            WHEN ABS(timing_error) < 5.0 THEN '00-05ms'
            WHEN ABS(timing_error) < 10.0 THEN '05-10ms'
            WHEN ABS(timing_error) < 15.0 THEN '10-15ms'
            WHEN ABS(timing_error) < 20.0 THEN '15-20ms'
            WHEN ABS(timing_error) < 25.0 THEN '20-25ms'
            WHEN ABS(timing_error) < 30.0 THEN '25-30ms'
            WHEN ABS(timing_error) < 40.0 THEN '30-40ms'
            ELSE '40ms+'
        END AS timing_band,
        CASE
            WHEN ABS(timing_error) < 5.0 THEN 1
            WHEN ABS(timing_error) < 10.0 THEN 2
            WHEN ABS(timing_error) < 15.0 THEN 3
            WHEN ABS(timing_error) < 20.0 THEN 4
            WHEN ABS(timing_error) < 25.0 THEN 5
            WHEN ABS(timing_error) < 30.0 THEN 6
            WHEN ABS(timing_error) < 40.0 THEN 7
            ELSE 8
        END AS sort_key,
        result,
        final_hla_deg
    FROM
        contact
)
SELECT
    timing_band,
    result,
    COUNT(*) AS n,
    ROUND(AVG(ABS(final_hla_deg)), 2) AS avg_abs_final_hla_deg,
    ROUND(
        100.0 * SUM(ABS(final_hla_deg) > 45.0) / COUNT(*),
        2
    ) AS hla_foul_zone_pct
FROM
    banded
GROUP BY
    timing_band,
    sort_key,
    result
ORDER BY
    sort_key,
    n DESC;