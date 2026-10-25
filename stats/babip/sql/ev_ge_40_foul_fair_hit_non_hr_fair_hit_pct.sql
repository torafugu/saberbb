SELECT
    adjusted_contact_type,
    COUNT(*) AS contacts,
    ROUND(AVG(launch_speed), 2) AS avg_ev_ms,
    ROUND(AVG(launch_angle), 2) AS avg_vla_deg,
    ROUND(
        100.0 * SUM(
            CASE
                WHEN launch_speed >= 40 THEN 1
                ELSE 0
            END
        ) / COUNT(*),
        2
    ) AS ev_ge_40_pct,
    ROUND(
        100.0 * SUM(
            CASE
                WHEN result = 'Foul' THEN 1
                ELSE 0
            END
        ) / COUNT(*),
        2
    ) AS foul_pct,
    ROUND(
        100.0 * SUM(
            CASE
                WHEN result IN ('Single', 'Double', 'Triple', 'HomeRun') THEN 1
                ELSE 0
            END
        ) / NULLIF(
            SUM(
                CASE
                    WHEN result IN (
                        'Single',
                        'Double',
                        'Triple',
                        'HomeRun',
                        'Out',
                        'DoublePlay',
                        'FieldersChoice'
                    ) THEN 1
                    ELSE 0
                END
            ),
            0
        ),
        2
    ) AS fair_hit_pct,
    ROUND(
        100.0 * SUM(
            CASE
                WHEN result IN ('Single', 'Double', 'Triple') THEN 1
                ELSE 0
            END
        ) / NULLIF(
            SUM(
                CASE
                    WHEN result IN (
                        'Single',
                        'Double',
                        'Triple',
                        'Out',
                        'DoublePlay',
                        'FieldersChoice'
                    ) THEN 1
                    ELSE 0
                END
            ),
            0
        ),
        2
    ) AS non_hr_fair_hit_pct
FROM
    player_game_batting
WHERE
    adjusted_contact_type IN (
        'SolidContact',
        'WeakContact',
        'MarginalContact'
    )
GROUP BY
    adjusted_contact_type
ORDER BY
    contacts DESC;