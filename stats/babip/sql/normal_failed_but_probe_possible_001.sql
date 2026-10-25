SELECT
    c.game_id,
    c.count_seq,
    c.fielder_id,
    c.fielder_position,
    c.selected,
    c.selected_path,
    c.end_reason,
    c.interception_catch_type,
    c.interception_error_type,
    c.last_evaluated_time_sec,
    c.air_probe_coarse_risk_ok_samples AS coarse_ok_samples,
    c.air_probe_risk_ok_samples AS fine_ok_samples,
    c.air_probe_coarse_first_catch_time_sec AS coarse_first_time_sec,
    c.air_probe_first_catch_time_sec AS fine_first_time_sec,
    c.air_probe_coarse_max_margin_sec AS coarse_max_margin_sec,
    c.air_probe_max_margin_sec AS fine_max_margin_sec,
    c.air_probe_coarse_max_margin_time_sec AS coarse_max_time_sec,
    c.air_probe_max_margin_time_sec AS fine_max_time_sec,
    c.air_probe_coarse_max_margin_height_m AS coarse_max_height_m,
    c.air_probe_max_margin_height_m AS fine_max_height_m,
    c.required_margin_sec,
    b.first_bounce_time,
    b.fence_impact_time,
    CASE
        WHEN c.air_probe_coarse_risk_ok_samples = 0 THEN 'sampling_gap'
        ELSE 'coarse_also_possible'
    END AS diagnosis
FROM
    player_game_fielding_candidate c
    JOIN player_game_batting b ON b.game_id = c.game_id
    AND b.count_seq = c.count_seq
WHERE
    b.result = 'Single'
    AND c.fielder_position IN ('LF', 'CF', 'RF')
    AND c.evaluated = 1
    AND c.risk_ok_samples = 0
    AND c.air_probe_risk_ok_samples > 0
    AND c.air_probe_step_sec = 0.01
    AND c.air_probe_coarse_step_sec = 0.05
    AND c.air_probe_coarse_risk_ok_samples IS NOT NULL
ORDER BY
    c.game_id,
    c.count_seq,
    c.fielder_position;