WITH batted AS (
    SELECT
        *
    FROM
        player_game_batting
    WHERE
        result IN (
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
    COUNT(*) AS all_contacts,
    SUM(result = 'Foul') AS fouls,
    ROUND(
        100.0 * SUM(result = 'Foul') / COUNT(*),
        2
    ) AS foul_pct_of_contacts
FROM
    batted;