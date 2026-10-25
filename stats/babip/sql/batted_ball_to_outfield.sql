SELECT
    game_id,
    count_seq,
    batter_id,
    adjusted_contact_type,
    launch_speed AS ev_ms,
    launch_angle AS vla_deg,
    final_hla_deg,
    polar_distance,
    polar_angle,
    total_time,
    first_bounce_distance,
    first_bounce_angle,
    first_bounce_time,
    fence_impact_time,
    fielder_position,
    result
FROM
    player_game_batting
WHERE
    result = 'Single'
    AND fielder_position IN ('LF', 'CF', 'RF')
    AND launch_speed >= 25
    AND launch_speed < 35
    AND launch_angle >= 25
    AND first_bounce_time IS NOT NULL
    AND fence_impact_time IS NULL
ORDER BY
    first_bounce_time DESC
LIMIT
    20;