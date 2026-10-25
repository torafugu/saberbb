WITH contact AS (
    SELECT
        *
    FROM
        player_game_batting
    WHERE
        final_hla_deg IS NOT NULL
)
SELECT
    CASE
        WHEN ABS(final_hla_deg) > 45.0
        AND result = 'Foul' THEN 'outside_and_foul'
        WHEN ABS(final_hla_deg) > 45.0
        AND result <> 'Foul' THEN 'outside_but_not_foul'
        WHEN ABS(final_hla_deg) <= 45.0
        AND result = 'Foul' THEN 'inside_but_foul'
        ELSE 'inside_and_fair'
    END AS classification,
    COUNT(*) AS n,
    ROUND(
        100.0 * COUNT(*) / SUM(COUNT(*)) OVER (),
        2
    ) AS pct
FROM
    contact
GROUP BY
    classification
ORDER BY
    n DESC;