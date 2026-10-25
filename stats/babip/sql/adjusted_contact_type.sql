SELECT
    adjusted_contact_type,
    COUNT(*) AS n,
    ROUND(
        COUNT(*) * 100.0 / SUM(COUNT(*)) OVER (),
        2
    ) AS pct
FROM
    player_game_batting
GROUP BY
    adjusted_contact_type
ORDER BY
    n DESC;