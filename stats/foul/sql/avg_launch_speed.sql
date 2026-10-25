WITH batted AS (
    SELECT
        *,
        length_offset_m / 0.16 AS normalized_tip_offset
    FROM
        player_game_batting
    WHERE
        length_offset_m >= 0.0
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
            WHEN normalized_tip_offset < 0.25 THEN '0.00-0.25'
            WHEN normalized_tip_offset < 0.50 THEN '0.25-0.50'
            WHEN normalized_tip_offset < 0.70 THEN '0.50-0.70'
            WHEN normalized_tip_offset < 0.80 THEN '0.70-0.80'
            WHEN normalized_tip_offset < 0.90 THEN '0.80-0.90'
            ELSE '0.90-1.00'
        END AS tip_band,
        CASE
            WHEN normalized_tip_offset < 0.25 THEN 1
            WHEN normalized_tip_offset < 0.50 THEN 2
            WHEN normalized_tip_offset < 0.70 THEN 3
            WHEN normalized_tip_offset < 0.80 THEN 4
            WHEN normalized_tip_offset < 0.90 THEN 5
            ELSE 6
        END AS sort_key
    FROM
        batted
    WHERE
        normalized_tip_offset < 1.0
)
SELECT
    tip_band,
    COUNT(*) AS contacts,
    ROUND(AVG(launch_speed), 2) AS avg_launch_speed,
    ROUND(
        AVG(
            CASE
                WHEN result <> 'Foul' THEN launch_speed
            END
        ),
        2
    ) AS avg_fair_launch_speed,
    ROUND(
        AVG(
            CASE
                WHEN result = 'Foul' THEN launch_speed
            END
        ),
        2
    ) AS avg_foul_launch_speed,
    ROUND(AVG(polar_distance), 2) AS avg_polar_distance,
    ROUND(AVG(ABS(final_hla_deg)), 2) AS avg_abs_hla
FROM
    banded
GROUP BY
    tip_band,
    sort_key
ORDER BY
    sort_key;