WITH base AS (
    SELECT
        *,
        length_offset_m / 0.16 AS normalized_tip_offset
    FROM
        player_game_batting
    WHERE
        length_offset_m >= 0.0
),
banded AS (
    SELECT
        *,
        CASE
            WHEN normalized_tip_offset < 0.50 THEN '0.00-0.50'
            WHEN normalized_tip_offset < 0.70 THEN '0.50-0.70'
            WHEN normalized_tip_offset < 0.80 THEN '0.70-0.80'
            WHEN normalized_tip_offset < 0.90 THEN '0.80-0.90'
            WHEN normalized_tip_offset < 1.00 THEN '0.90-1.00'
            WHEN normalized_tip_offset < 1.23125 THEN '1.00-1.23'
            ELSE '1.23+'
        END AS tip_band,
        CASE
            WHEN normalized_tip_offset < 0.50 THEN 1
            WHEN normalized_tip_offset < 0.70 THEN 2
            WHEN normalized_tip_offset < 0.80 THEN 3
            WHEN normalized_tip_offset < 0.90 THEN 4
            WHEN normalized_tip_offset < 1.00 THEN 5
            WHEN normalized_tip_offset < 1.23125 THEN 6
            ELSE 7
        END AS sort_key
    FROM
        base
)
SELECT
    tip_band,
    original_contact_type,
    adjusted_contact_type,
    COUNT(*) AS n,
    ROUND(
        100.0 * COUNT(*) / SUM(COUNT(*)) OVER (
            PARTITION BY tip_band,
            original_contact_type
        ),
        2
    ) AS pct_within_original
FROM
    banded
GROUP BY
    tip_band,
    sort_key,
    original_contact_type,
    adjusted_contact_type
ORDER BY
    sort_key,
    original_contact_type,
    n DESC;