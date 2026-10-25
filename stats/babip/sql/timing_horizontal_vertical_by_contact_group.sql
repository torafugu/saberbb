SELECT
    CASE
        WHEN b.length_offset_m >= 0.16 THEN 'length_miss'
        WHEN b.adjusted_contact_type = 'SwungAndMiss' THEN 'thickness_miss'
        ELSE 'contact'
    END AS contact_group,
    COUNT(*) AS n,
    ROUND(AVG(ABS(p.timing_offset_sec)) * 1000, 3) AS avg_abs_timing_ms,
    ROUND(AVG(ABS(p.horizontal_offset_m)) * 100, 2) AS avg_abs_horizontal_cm,
    ROUND(AVG(ABS(p.vertical_offset_m)) * 100, 2) AS avg_abs_vertical_cm
FROM
    player_game_batting b
    JOIN player_game_pitching p ON p.game_id = b.game_id
    AND p.count_seq = b.count_seq
WHERE
    b.adjusted_contact_type <> 'Take'
GROUP BY
    contact_group;