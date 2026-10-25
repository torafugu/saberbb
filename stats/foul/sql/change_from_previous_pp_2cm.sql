WITH contacts AS (
    SELECT
        *,
        CAST(length_offset_m / 0.02 AS INTEGER) AS bin_no
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
)
SELECT
    ROUND(bin_no * 0.02, 2) AS offset_from_m,
    ROUND((bin_no + 1) * 0.02, 2) AS offset_to_m,
    COUNT(*) AS contacts,
    SUM(result = 'Foul') AS fouls,
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
        100.0 * SUM(
            adjusted_contact_type IN (
                'MarginalContact',
                'FoulTip'
            )
        ) / COUNT(*),
        2
    ) AS marginal_pct
FROM
    contacts
GROUP BY
    bin_no
ORDER BY
    bin_no;