SELECT
    CASE
        WHEN ABS(p.timing_offset_sec) < 0.002 THEN '0-2ms'
        WHEN ABS(p.timing_offset_sec) < 0.004 THEN '2-4ms'
        WHEN ABS(p.timing_offset_sec) < 0.006 THEN '4-6ms'
        WHEN ABS(p.timing_offset_sec) < 0.008 THEN '6-8ms'
        WHEN ABS(p.timing_offset_sec) < 0.010 THEN '8-10ms'
        WHEN ABS(p.timing_offset_sec) < 0.015 THEN '10-15ms'
        ELSE '15ms+'
    END AS timing_band,
    b.adjusted_contact_type,
    COUNT(*) AS n
FROM
    player_game_batting b
    JOIN player_game_pitching p ON p.game_id = b.game_id
    AND p.count_seq = b.count_seq
WHERE
    b.adjusted_contact_type <> 'Take'
GROUP BY
    timing_band,
    b.adjusted_contact_type
ORDER BY
    timing_band,
    b.adjusted_contact_type;