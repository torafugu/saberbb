WITH tip_contacts AS (
    SELECT
        *,
        length_offset_m / 0.16 AS normalized_tip_offset
    FROM
        player_game_batting
    WHERE
        length_offset_m >= 0.16 * 0.90
        AND length_offset_m < 0.16
        AND adjusted_contact_type <> 'SwungAndMiss'
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
    COUNT(*) AS contacts,
    SUM(result = 'Foul') AS fouls,
    SUM(
        result IN (
            'Out',
            'Single',
            'Double',
            'Triple',
            'HomeRun',
            'DoublePlay',
            'FieldersChoice'
        )
    ) AS fair_balls,
    ROUND(
        100.0 * SUM(result = 'Foul') / COUNT(*),
        2
    ) AS foul_pct,
    ROUND(
        100.0 * SUM(result <> 'Foul') / COUNT(*),
        2
    ) AS fair_pct,
    ROUND(AVG(launch_speed), 2) AS avg_launch_speed,
    ROUND(AVG(ABS(final_hla_deg)), 2) AS avg_abs_hla
FROM
    tip_contacts;