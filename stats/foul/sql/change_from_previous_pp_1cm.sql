WITH contacts AS (
    SELECT
        *,
        CAST(length_offset_m / 0.01 AS INTEGER) AS bin_no
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
rates AS (
    SELECT
        bin_no,
        bin_no * 0.01 AS offset_from_m,
        (bin_no + 1) * 0.01 AS offset_to_m,
        COUNT(*) AS contacts,
        SUM(result = 'Foul') AS fouls,
        100.0 * SUM(result = 'Foul') / COUNT(*) AS foul_pct
    FROM
        contacts
    GROUP BY
        bin_no
),
compared AS (
    SELECT
        *,
        LAG(foul_pct) OVER (
            ORDER BY
                bin_no
        ) AS previous_foul_pct
    FROM
        rates
)
SELECT
    ROUND(offset_from_m, 2) AS offset_from_m,
    ROUND(offset_to_m, 2) AS offset_to_m,
    contacts,
    fouls,
    ROUND(foul_pct, 2) AS foul_pct,
    ROUND(
        foul_pct - previous_foul_pct,
        2
    ) AS change_from_previous_pp,
    CASE
        WHEN contacts < 30 THEN 'small_sample'
        WHEN ABS(
            foul_pct - previous_foul_pct
        ) >= 15.0 THEN 'large_jump'
        ELSE 'ok'
    END AS check_result
FROM
    compared
ORDER BY
    bin_no;