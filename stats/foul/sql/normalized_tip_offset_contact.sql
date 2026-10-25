normalized_tip_offsetWITH base AS (
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
            WHEN normalized_tip_offset < 0.25 THEN '0.00-0.25'
            WHEN normalized_tip_offset < 0.50 THEN '0.25-0.50'
            WHEN normalized_tip_offset < 0.70 THEN '0.50-0.70'
            WHEN normalized_tip_offset < 0.80 THEN '0.70-0.80'
            WHEN normalized_tip_offset < 0.90 THEN '0.80-0.90'
            WHEN normalized_tip_offset < 1.00 THEN '0.90-1.00'
            WHEN normalized_tip_offset < 1.23125 THEN '1.00-1.23'
            ELSE '1.23+'
        END AS tip_band,
        CASE
            WHEN normalized_tip_offset < 0.25 THEN 1
            WHEN normalized_tip_offset < 0.50 THEN 2
            WHEN normalized_tip_offset < 0.70 THEN 3
            WHEN normalized_tip_offset < 0.80 THEN 4
            WHEN normalized_tip_offset < 0.90 THEN 5
            WHEN normalized_tip_offset < 1.00 THEN 6
            WHEN normalized_tip_offset < 1.23125 THEN 7
            ELSE 8
        END AS sort_key
    FROM
        base
    WHERE
        original_contact_type IN (
            'SolidContact',
            'WeakContact',
            'MarginalContact',
            'FoulTip',
            'SwungAndMiss'
        )
)
SELECT
    tip_band,
    COUNT(*) AS attempts,
    SUM(adjusted_contact_type <> 'SwungAndMiss') AS contacts,
    SUM(adjusted_contact_type = 'SolidContact') AS solid_n,
    SUM(adjusted_contact_type = 'WeakContact') AS weak_n,
    SUM(
        adjusted_contact_type IN (
            'MarginalContact',
            'FoulTip'
        )
    ) AS marginal_n,
    SUM(adjusted_contact_type = 'SwungAndMiss') AS miss_n,
    ROUND(
        100.0 * SUM(adjusted_contact_type = 'SolidContact') / NULLIF(
            SUM(adjusted_contact_type <> 'SwungAndMiss'),
            0
        ),
        2
    ) AS solid_pct_of_contacts,
    ROUND(
        100.0 * SUM(adjusted_contact_type = 'WeakContact') / NULLIF(
            SUM(adjusted_contact_type <> 'SwungAndMiss'),
            0
        ),
        2
    ) AS weak_pct_of_contacts,
    ROUND(
        100.0 * SUM(
            adjusted_contact_type IN (
                'MarginalContact',
                'FoulTip'
            )
        ) / NULLIF(
            SUM(adjusted_contact_type <> 'SwungAndMiss'),
            0
        ),
        2
    ) AS marginal_pct_of_contacts,
    ROUND(
        100.0 * SUM(adjusted_contact_type = 'SwungAndMiss') / COUNT(*),
        2
    ) AS miss_pct
FROM
    banded
GROUP BY
    tip_band,
    sort_key
ORDER BY
    sort_key;