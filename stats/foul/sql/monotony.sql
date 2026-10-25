WITH base AS (
    SELECT
        *,
        length_offset_m / 0.16 AS normalized_tip_offset
    FROM
        player_game_batting
    WHERE
        length_offset_m >= 0.0
        AND original_contact_type IN (
            'SolidContact',
            'WeakContact',
            'MarginalContact',
            'FoulTip',
            'SwungAndMiss'
        )
),
banded AS (
    SELECT
        *,
        CASE
            WHEN normalized_tip_offset < 0.25 THEN 1
            WHEN normalized_tip_offset < 0.50 THEN 2
            WHEN normalized_tip_offset < 0.70 THEN 3
            WHEN normalized_tip_offset < 0.80 THEN 4
            WHEN normalized_tip_offset < 0.90 THEN 5
            WHEN normalized_tip_offset < 1.00 THEN 6
            WHEN normalized_tip_offset < 1.23125 THEN 7
            ELSE 8
        END AS band_no
    FROM
        base
),
rates AS (
    SELECT
        band_no,
        COUNT(*) AS attempts,
        SUM(adjusted_contact_type <> 'SwungAndMiss') AS contacts,
        100.0 * SUM(adjusted_contact_type = 'SolidContact') / NULLIF(
            SUM(adjusted_contact_type <> 'SwungAndMiss'),
            0
        ) AS solid_pct,
        100.0 * SUM(
            adjusted_contact_type IN (
                'WeakContact',
                'MarginalContact',
                'FoulTip'
            )
        ) / NULLIF(
            SUM(adjusted_contact_type <> 'SwungAndMiss'),
            0
        ) AS weak_or_marginal_pct
    FROM
        banded
    GROUP BY
        band_no
),
compared AS (
    SELECT
        *,
        LAG(solid_pct) OVER (
            ORDER BY
                band_no
        ) AS previous_solid_pct,
        LAG(weak_or_marginal_pct) OVER (
            ORDER BY
                band_no
        ) AS previous_weak_or_marginal_pct
    FROM
        rates
)
SELECT
    band_no,
    attempts,
    contacts,
    ROUND(solid_pct, 2) AS solid_pct,
    ROUND(
        solid_pct - previous_solid_pct,
        2
    ) AS solid_change_pp,
    ROUND(weak_or_marginal_pct, 2) AS weak_or_marginal_pct,
    ROUND(
        weak_or_marginal_pct - previous_weak_or_marginal_pct,
        2
    ) AS weak_or_marginal_change_pp,
    CASE
        WHEN solid_pct > previous_solid_pct + 3.0 THEN 'solid_increase'
        WHEN weak_or_marginal_pct < previous_weak_or_marginal_pct - 3.0 THEN 'weak_marginal_decrease'
        ELSE 'ok'
    END AS check_result
FROM
    compared
ORDER BY
    band_no;