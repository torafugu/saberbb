WITH tip_contacts AS (
    SELECT
        *,
        length_offset_m / 0.16 AS normalized_tip_offset
    FROM
        player_game_batting
    WHERE
        length_offset_m >= 0.16 * 0.90
        AND length_offset_m < 0.16
        AND adjusted_contact_type IN (
            'SolidContact',
            'WeakContact',
            'MarginalContact'
        )
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
)
SELECT
    adjusted_contact_type,
    COUNT(*) AS contacts,
    SUM(result = 'Foul') AS fouls,
    SUM(result <> 'Foul') AS fair_balls,
    ROUND(
        100.0 * SUM(result = 'Foul') / COUNT(*),
        2
    ) AS foul_pct,
    ROUND(
        100.0 * SUM(result <> 'Foul') / COUNT(*),
        2
    ) AS fair_pct,
    ROUND(AVG(launch_speed), 2) AS avg_launch_speed,
    ROUND(AVG(ABS(final_hla_deg)), 2) AS avg_abs_hla,
    ROUND(AVG(launch_angle), 2) AS avg_launch_angle
FROM
    tip_contacts
GROUP BY
    adjusted_contact_type
ORDER BY
    CASE
        adjusted_contact_type
        WHEN 'SolidContact' THEN 1
        WHEN 'WeakContact' THEN 2
        WHEN 'MarginalContact' THEN 3
        ELSE 4
    END;