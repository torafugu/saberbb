SELECT
    adjusted_contact_type,
    result,
    COUNT(*) AS n
FROM
    player_game_batting
GROUP BY
    adjusted_contact_type,
    result
ORDER BY
    adjusted_contact_type,
    n DESC;