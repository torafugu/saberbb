SELECT
    original_contact_type,
    adjusted_contact_type,
    COUNT(*) AS n
FROM
    player_game_batting
GROUP BY
    original_contact_type,
    adjusted_contact_type
ORDER BY
    original_contact_type,
    adjusted_contact_type;