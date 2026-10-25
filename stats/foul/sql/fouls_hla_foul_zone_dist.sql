WITH contact AS (
    SELECT
        timing_error,
        bat_speed,
        timing_angle_error,
        base_hla_deg,
        final_hla_deg,
        result
    FROM
        player_game_batting
    WHERE
        final_hla_deg IS NOT NULL
)
SELECT
    COUNT(*) AS contacts,
    SUM(result = 'Foul') AS fouls,
    ROUND(
        100.0 * SUM(result = 'Foul') / COUNT(*),
        2
    ) AS foul_pct_of_contacts,
    SUM(ABS(final_hla_deg) > 45.0) AS hla_foul_zone,
    ROUND(
        100.0 * SUM(ABS(final_hla_deg) > 45.0) / COUNT(*),
        2
    ) AS hla_foul_zone_pct
FROM
    contact;