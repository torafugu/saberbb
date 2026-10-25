SELECT
    c.game_id,
    c.count_seq,
    c.fielder_position,
    c.fielder_id,
    c.lane_eligible,
    c.distance_eligible,
    c.end_reason,
    c.selected,
    c.selected_path,
    b.result,
    c.air_probe_risk_ok_samples,
    ROUND(c.air_probe_first_catch_time_sec, 4) AS first_catch_time_sec,
    ROUND(c.air_probe_first_catch_height_m, 4) AS first_catch_height_m,
    ROUND(c.air_probe_first_catch_margin_sec, 4) AS first_catch_margin_sec
FROM
    player_game_fielding_candidate AS c
    JOIN player_game_batting AS b ON b.game_id = c.game_id
    AND b.count_seq = c.count_seq
WHERE
    c.fielder_position IN ('LF', 'CF', 'RF')
    AND c.evaluated = 0
    AND c.air_probe_risk_ok_samples > 0
ORDER BY
    c.game_id,
    c.count_seq,
    c.fielder_position;