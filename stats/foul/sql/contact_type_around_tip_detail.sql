WITH tip_contacts AS (
    SELECT
        *,
        length_offset_m / 0.16 AS normalized_tip_offset
    FROM
        player_game_batting
    WHERE
        length_offset_m >= 0.16 * 0.80
        AND length_offset_m < 0.16
        AND result IN (
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
        *,
        CASE
            WHEN normalized_tip_offset < 0.90 THEN '0.80-0.90'
            WHEN normalized_tip_offset < 0.95 THEN '0.90-0.95'
            ELSE '0.95-1.00'
        END AS tip_band,
        CASE
            WHEN normalized_tip_offset < 0.90 THEN 1
            WHEN normalized_tip_offset < 0.95 THEN 2
            ELSE 3
        END AS sort_key
    FROM
        tip_contacts
)
SELECT
    tip_band,
    COUNT(*) AS contacts,
    SUM(result = 'Foul') AS fouls,
    SUM(result <> 'Foul') AS fair_balls,
    ROUND(
        100.0 * SUM(result = 'Foul') / COUNT(*),
        2
    ) AS foul_pct,
    ROUND(
        100.0 * SUM(adjusted_contact_type = 'SolidContact') / COUNT(*),
        2
    ) AS solid_pct,
    ROUND(
        100.0 * SUM(adjusted_contact_type = 'WeakContact') / COUNT(*),
        2
    ) AS weak_pct,
    ROUND(
        100.0 * SUM(adjusted_contact_type = 'MarginalContact') / COUNT(*),
        2
    ) AS marginal_pct,
    ROUND(AVG(launch_speed), 2) AS avg_launch_speed,
    ROUND(AVG(ABS(final_hla_deg)), 2) AS avg_abs_hla
FROM
    banded
GROUP BY
    tip_band,
    sort_key
ORDER BY
    sort_key;