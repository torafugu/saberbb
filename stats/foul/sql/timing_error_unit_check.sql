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
    COUNT(*) AS n,
    MIN(timing_error) AS min_raw,
    MAX(timing_error) AS max_raw,
    ROUND(AVG(timing_error), 6) AS avg_raw,
    ROUND(AVG(ABS(timing_error)), 6) AS avg_abs_raw
FROM
    batted;