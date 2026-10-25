WITH banded AS (
    SELECT
        adjusted_contact_type,
        CASE
            WHEN launch_angle < -10 THEN 1
            WHEN launch_angle < 0 THEN 2
            WHEN launch_angle < 10 THEN 3
            WHEN launch_angle < 25 THEN 4
            WHEN launch_angle < 40 THEN 5
            ELSE 6
        END AS band_order,
        CASE
            WHEN launch_angle < -10 THEN '<-10'
            WHEN launch_angle < 0 THEN '-10..0'
            WHEN launch_angle < 10 THEN '0..10'
            WHEN launch_angle < 25 THEN '10..25'
            WHEN launch_angle < 40 THEN '25..40'
            ELSE '40+'
        END AS vla_band
    FROM
        player_game_batting
    WHERE
        adjusted_contact_type IN (
            'SolidContact',
            'WeakContact',
            'MarginalContact'
        )
),
counts AS (
    SELECT
        adjusted_contact_type,
        band_order,
        vla_band,
        COUNT(*) AS n
    FROM
        banded
    GROUP BY
        adjusted_contact_type,
        band_order,
        vla_band
)
SELECT
    adjusted_contact_type,
    vla_band,
    n,
    ROUND(
        100.0 * n / SUM(n) OVER (PARTITION BY adjusted_contact_type),
        2
    ) AS pct_within_type
FROM
    counts
ORDER BY
    adjusted_contact_type,
    band_order;