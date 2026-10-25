SELECT
    c.game_id,
    c.count_seq,
    c.fielder_position,
    c.fielder_id,
    c.lane_eligible,
    c.distance_eligible,
    c.evaluated,
    c.air_samples,
    c.height_ok_samples,
    c.reachable_samples,
    c.risk_ok_samples,
    ROUND(c.max_air_margin_sec, 4) AS max_air_margin_sec,
    c.required_margin_sec,
    ROUND(c.max_air_margin_time_sec, 4) AS max_air_margin_time_sec,
    ROUND(c.max_air_margin_height_m, 4) AS max_air_margin_height_m,
    c.last_evaluated_time_sec,
    c.end_reason,
    c.interception_catch_type,
    c.interception_error_type,
    c.interception_time_sec,
    c.selected,
    c.selected_path,
    b.first_bounce_time,
    b.fence_impact_time,
    b.result
FROM
    player_game_fielding_candidate AS c
    LEFT JOIN player_game_batting AS b ON b.game_id = c.game_id
    AND b.count_seq = c.count_seq
WHERE
    c.game_id = 16075
    AND c.count_seq = 185
ORDER BY
    c.fielder_position;