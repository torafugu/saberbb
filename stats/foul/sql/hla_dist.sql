WITH contact AS (
    SELECT
        *
    FROM
        player_game_batting
    WHERE
        final_hla_deg IS NOT NULL
),
banded AS (
    SELECT
        CASE
            WHEN ABS(final_hla_deg) < 15.0 THEN '00-15'
            WHEN ABS(final_hla_deg) < 30.0 THEN '15-30'
            WHEN ABS(final_hla_deg) < 40.0 THEN '30-40'
            WHEN ABS(final_hla_deg) < 45.0 THEN '40-45'
            WHEN ABS(final_hla_deg) < 60.0 THEN '45-60'
            WHEN ABS(final_hla_deg) < 90.0 THEN '60-90'
            ELSE '90+'
        END AS hla_band,
        result
    FROM
        contact
)
SELECT
    hla_band,
    result,
    COUNT(*) AS n,
    ROUND(
        100.0 * COUNT(*) / SUM(COUNT(*)) OVER (
            PARTITION BY hla_band
        ),
        2
    ) AS pct_within_band
FROM
    banded
GROUP BY
    hla_band,
    result
ORDER BY
    CASE
        hla_band
        WHEN '00-15' THEN 1
        WHEN '15-30' THEN 2
        WHEN '30-40' THEN 3
        WHEN '40-45' THEN 4
        WHEN '45-60' THEN 5
        WHEN '60-90' THEN 6
        ELSE 7
    END,
    n DESC;