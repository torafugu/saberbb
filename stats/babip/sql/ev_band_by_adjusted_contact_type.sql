WITH banded AS (
    SELECT
        adjusted_contact_type,
        CASE
            WHEN launch_speed < 25 THEN 1
            WHEN launch_speed < 35 THEN 2
            WHEN launch_speed < 40 THEN 3
            WHEN launch_speed < 45 THEN 4
            WHEN launch_speed < 50 THEN 5
            ELSE 6
        END AS band_order,
        CASE
            WHEN launch_speed < 25 THEN '<25'
            WHEN launch_speed < 35 THEN '25-35'
            WHEN launch_speed < 40 THEN '35-40'
            WHEN launch_speed < 45 THEN '40-45'
            WHEN launch_speed < 50 THEN '45-50'
            ELSE '50+'
        END AS ev_band
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
        ev_band,
        COUNT(*) AS n
    FROM
        banded
    GROUP BY
        adjusted_contact_type,
        band_order,
        ev_band
)
SELECT
    adjusted_contact_type,
    ev_band,
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