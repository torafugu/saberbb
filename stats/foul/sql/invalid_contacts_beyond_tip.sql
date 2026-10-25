SELECT
    COUNT(*) AS invalid_contacts_beyond_tip
FROM
    player_game_batting
WHERE
    length_offset_m > 0.16
    AND adjusted_contact_type <> 'SwungAndMiss';