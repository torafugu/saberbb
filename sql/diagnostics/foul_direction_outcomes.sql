-- Read-only SQLite diagnostics. Bind :first_game_id and :last_game_id (inclusive).
-- Use a single cohort per run. seq=1 keeps one row per contact even with extra throws.
-- Direction labels describe recorded angles, not a rulebook fair/foul ruling.
-- resolution_hint is inferred from current code and saved fields, not a recorded branch trace.

-- 1. Coverage and direction/result gap by contact type and model version
WITH contacts AS (
 SELECT b.*, COALESCE(b.hla_model_version,1) AS hla_version,
        f.seq AS fielding_seq, f.catch_fielder_id, f.catch_fielder_position,
        f.interception_fielder_position, f.selection_path, f.catch_type, f.error_type,
        f.catch_time_sec, f.catch_z_m, f.waiting_time_sec, f.play_type
 FROM player_game_batting b
 LEFT JOIN player_game_fielding f
   ON f.game_id=b.game_id AND f.count_seq=b.count_seq AND f.seq=1
 WHERE b.game_id BETWEEN :first_game_id AND :last_game_id
   AND b.adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
), directions AS (
 SELECT *,
   CASE WHEN final_hla_deg IS NULL THEN 'unknown'
        WHEN ABS(final_hla_deg)>=90 THEN 'backward'
        WHEN ABS(final_hla_deg)>=45 THEN 'foul_side' ELSE 'fair' END AS launch_direction,
   CASE WHEN first_bounce_angle IS NULL THEN 'unknown'
        WHEN ABS(first_bounce_angle)>=90 THEN 'backward'
        WHEN ABS(first_bounce_angle)>=45 THEN 'foul_side' ELSE 'fair' END AS bounce_direction,
   CASE WHEN polar_angle IS NULL THEN 'unknown'
        WHEN ABS(polar_angle)>=90 THEN 'backward'
        WHEN ABS(polar_angle)>=45 THEN 'foul_side' ELSE 'fair' END AS final_direction,
   CASE WHEN fielding_seq IS NULL THEN 'missing_row'
        WHEN catch_type IS NULL THEN 'missing_catch_diagnostics' ELSE 'recorded' END AS fielding_coverage,
   CASE WHEN outbound_result='Foul' THEN 'outbound_foul'
        WHEN outbound_result='HomeRun' THEN 'outbound_home_run'
        WHEN outbound_result='GroundRuleDouble' THEN 'outbound_ground_rule_double'
        WHEN outbound_result IS NULL OR outbound_result<>'InField' THEN 'unknown_outbound'
        WHEN catch_type='DirectFly' THEN 'direct_fly_before_final_foul_check'
        WHEN ABS(polar_angle)>=45 THEN 'final_position_foul_check'
        ELSE 'fair_or_missing_fly_diagnostics' END AS resolution_hint,
   CASE WHEN catch_type<>'DirectFly' OR catch_type IS NULL THEN 'not_direct_fly'
        WHEN catch_time_sec IS NULL THEN 'missing_catch_time'
        WHEN (first_bounce_time IS NOT NULL AND catch_time_sec>=first_bounce_time-1e-9)
          OR (fence_impact_time IS NOT NULL AND catch_time_sec>=fence_impact_time-1e-9)
          THEN 'at_or_after_event_boundary'
        WHEN first_bounce_time IS NULL AND fence_impact_time IS NULL THEN 'no_recorded_boundary'
        ELSE 'before_recorded_boundaries' END AS fly_boundary_status
 FROM contacts
), diagnosed AS (
 SELECT *,
   CASE WHEN launch_direction='unknown' OR final_direction='unknown' THEN 'unknown_direction'
        WHEN launch_direction IN ('foul_side','backward') AND result<>'Foul' THEN 'launch_foul_non_foul_result'
        WHEN launch_direction='fair' AND result='Foul' THEN 'launch_fair_foul_result'
        ELSE 'direction_result_agree' END AS discrepancy
 FROM directions
)
SELECT hla_version,adjusted_contact_type,COUNT(*) AS contacts,
 SUM(CASE WHEN launch_direction IN ('foul_side','backward') THEN 1 ELSE 0 END) AS launch_foul_directions,
 SUM(CASE WHEN launch_direction='backward' THEN 1 ELSE 0 END) AS launch_backward_directions,
 SUM(CASE WHEN final_direction IN ('foul_side','backward') THEN 1 ELSE 0 END) AS final_foul_directions,
 SUM(CASE WHEN result='Foul' THEN 1 ELSE 0 END) AS result_fouls,
 SUM(CASE WHEN discrepancy='launch_foul_non_foul_result' THEN 1 ELSE 0 END) AS launch_foul_non_foul_results,
 SUM(CASE WHEN discrepancy='launch_fair_foul_result' THEN 1 ELSE 0 END) AS launch_fair_foul_results,
 SUM(CASE WHEN fielding_coverage='missing_row' THEN 1 ELSE 0 END) AS missing_fielding_rows,
 SUM(CASE WHEN fielding_coverage='missing_catch_diagnostics' THEN 1 ELSE 0 END) AS missing_catch_diagnostics,
 SUM(CASE WHEN discrepancy='unknown_direction' THEN 1 ELSE 0 END) AS unknown_directions
FROM diagnosed GROUP BY hla_version,adjusted_contact_type
ORDER BY hla_version,adjusted_contact_type;

-- 2. Full transition matrix; sums to the contact count, including missing fielding rows
WITH contacts AS (
 SELECT b.*, COALESCE(b.hla_model_version,1) AS hla_version,
        f.seq AS fielding_seq, f.catch_fielder_id, f.catch_fielder_position,
        f.interception_fielder_position, f.selection_path, f.catch_type, f.error_type,
        f.catch_time_sec, f.catch_z_m, f.waiting_time_sec, f.play_type
 FROM player_game_batting b
 LEFT JOIN player_game_fielding f
   ON f.game_id=b.game_id AND f.count_seq=b.count_seq AND f.seq=1
 WHERE b.game_id BETWEEN :first_game_id AND :last_game_id
   AND b.adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
), directions AS (
 SELECT *,
   CASE WHEN final_hla_deg IS NULL THEN 'unknown'
        WHEN ABS(final_hla_deg)>=90 THEN 'backward'
        WHEN ABS(final_hla_deg)>=45 THEN 'foul_side' ELSE 'fair' END AS launch_direction,
   CASE WHEN first_bounce_angle IS NULL THEN 'unknown'
        WHEN ABS(first_bounce_angle)>=90 THEN 'backward'
        WHEN ABS(first_bounce_angle)>=45 THEN 'foul_side' ELSE 'fair' END AS bounce_direction,
   CASE WHEN polar_angle IS NULL THEN 'unknown'
        WHEN ABS(polar_angle)>=90 THEN 'backward'
        WHEN ABS(polar_angle)>=45 THEN 'foul_side' ELSE 'fair' END AS final_direction,
   CASE WHEN fielding_seq IS NULL THEN 'missing_row'
        WHEN catch_type IS NULL THEN 'missing_catch_diagnostics' ELSE 'recorded' END AS fielding_coverage,
   CASE WHEN outbound_result='Foul' THEN 'outbound_foul'
        WHEN outbound_result='HomeRun' THEN 'outbound_home_run'
        WHEN outbound_result='GroundRuleDouble' THEN 'outbound_ground_rule_double'
        WHEN outbound_result IS NULL OR outbound_result<>'InField' THEN 'unknown_outbound'
        WHEN catch_type='DirectFly' THEN 'direct_fly_before_final_foul_check'
        WHEN ABS(polar_angle)>=45 THEN 'final_position_foul_check'
        ELSE 'fair_or_missing_fly_diagnostics' END AS resolution_hint,
   CASE WHEN catch_type<>'DirectFly' OR catch_type IS NULL THEN 'not_direct_fly'
        WHEN catch_time_sec IS NULL THEN 'missing_catch_time'
        WHEN (first_bounce_time IS NOT NULL AND catch_time_sec>=first_bounce_time-1e-9)
          OR (fence_impact_time IS NOT NULL AND catch_time_sec>=fence_impact_time-1e-9)
          THEN 'at_or_after_event_boundary'
        WHEN first_bounce_time IS NULL AND fence_impact_time IS NULL THEN 'no_recorded_boundary'
        ELSE 'before_recorded_boundaries' END AS fly_boundary_status
 FROM contacts
), diagnosed AS (
 SELECT *,
   CASE WHEN launch_direction='unknown' OR final_direction='unknown' THEN 'unknown_direction'
        WHEN launch_direction IN ('foul_side','backward') AND result<>'Foul' THEN 'launch_foul_non_foul_result'
        WHEN launch_direction='fair' AND result='Foul' THEN 'launch_fair_foul_result'
        ELSE 'direction_result_agree' END AS discrepancy
 FROM directions
)
SELECT hla_version,adjusted_contact_type,launch_direction,bounce_direction,final_direction,
 outbound_result,result,fielding_coverage,selection_path,catch_type,error_type,play_type,
 catch_fielder_position,resolution_hint,fly_boundary_status,COUNT(*) AS contacts,
 ROUND(AVG(catch_time_sec),4) AS avg_catch_time_sec,
 ROUND(AVG(catch_z_m),4) AS avg_catch_z_m
FROM diagnosed
GROUP BY hla_version,adjusted_contact_type,launch_direction,bounce_direction,final_direction,
 outbound_result,result,fielding_coverage,selection_path,catch_type,error_type,play_type,
 catch_fielder_position,resolution_hint,fly_boundary_status
ORDER BY hla_version,adjusted_contact_type,contacts DESC;

-- 3. Exhaustive gap breakdown (no row limit); these groups explain query 1 gap counts
WITH contacts AS (
 SELECT b.*, COALESCE(b.hla_model_version,1) AS hla_version,
        f.seq AS fielding_seq, f.catch_fielder_id, f.catch_fielder_position,
        f.interception_fielder_position, f.selection_path, f.catch_type, f.error_type,
        f.catch_time_sec, f.catch_z_m, f.waiting_time_sec, f.play_type
 FROM player_game_batting b
 LEFT JOIN player_game_fielding f
   ON f.game_id=b.game_id AND f.count_seq=b.count_seq AND f.seq=1
 WHERE b.game_id BETWEEN :first_game_id AND :last_game_id
   AND b.adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
), directions AS (
 SELECT *,
   CASE WHEN final_hla_deg IS NULL THEN 'unknown'
        WHEN ABS(final_hla_deg)>=90 THEN 'backward'
        WHEN ABS(final_hla_deg)>=45 THEN 'foul_side' ELSE 'fair' END AS launch_direction,
   CASE WHEN first_bounce_angle IS NULL THEN 'unknown'
        WHEN ABS(first_bounce_angle)>=90 THEN 'backward'
        WHEN ABS(first_bounce_angle)>=45 THEN 'foul_side' ELSE 'fair' END AS bounce_direction,
   CASE WHEN polar_angle IS NULL THEN 'unknown'
        WHEN ABS(polar_angle)>=90 THEN 'backward'
        WHEN ABS(polar_angle)>=45 THEN 'foul_side' ELSE 'fair' END AS final_direction,
   CASE WHEN fielding_seq IS NULL THEN 'missing_row'
        WHEN catch_type IS NULL THEN 'missing_catch_diagnostics' ELSE 'recorded' END AS fielding_coverage,
   CASE WHEN outbound_result='Foul' THEN 'outbound_foul'
        WHEN outbound_result='HomeRun' THEN 'outbound_home_run'
        WHEN outbound_result='GroundRuleDouble' THEN 'outbound_ground_rule_double'
        WHEN outbound_result IS NULL OR outbound_result<>'InField' THEN 'unknown_outbound'
        WHEN catch_type='DirectFly' THEN 'direct_fly_before_final_foul_check'
        WHEN ABS(polar_angle)>=45 THEN 'final_position_foul_check'
        ELSE 'fair_or_missing_fly_diagnostics' END AS resolution_hint,
   CASE WHEN catch_type<>'DirectFly' OR catch_type IS NULL THEN 'not_direct_fly'
        WHEN catch_time_sec IS NULL THEN 'missing_catch_time'
        WHEN (first_bounce_time IS NOT NULL AND catch_time_sec>=first_bounce_time-1e-9)
          OR (fence_impact_time IS NOT NULL AND catch_time_sec>=fence_impact_time-1e-9)
          THEN 'at_or_after_event_boundary'
        WHEN first_bounce_time IS NULL AND fence_impact_time IS NULL THEN 'no_recorded_boundary'
        ELSE 'before_recorded_boundaries' END AS fly_boundary_status
 FROM contacts
), diagnosed AS (
 SELECT *,
   CASE WHEN launch_direction='unknown' OR final_direction='unknown' THEN 'unknown_direction'
        WHEN launch_direction IN ('foul_side','backward') AND result<>'Foul' THEN 'launch_foul_non_foul_result'
        WHEN launch_direction='fair' AND result='Foul' THEN 'launch_fair_foul_result'
        ELSE 'direction_result_agree' END AS discrepancy
 FROM directions
)
SELECT hla_version,adjusted_contact_type,discrepancy,launch_direction,final_direction,
 outbound_result,result,fielding_coverage,selection_path,catch_type,error_type,
 catch_fielder_position,resolution_hint,fly_boundary_status,COUNT(*) AS contacts,
 MIN(game_id) AS first_game_id,MAX(game_id) AS last_game_id
FROM diagnosed
WHERE discrepancy<>'direction_result_agree'
GROUP BY hla_version,adjusted_contact_type,discrepancy,launch_direction,final_direction,
 outbound_result,result,fielding_coverage,selection_path,catch_type,error_type,
 catch_fielder_position,resolution_hint,fly_boundary_status
ORDER BY contacts DESC,hla_version,adjusted_contact_type;

-- 4. Individual gaps (first 200 by game/count, not the full population)
WITH contacts AS (
 SELECT b.*, COALESCE(b.hla_model_version,1) AS hla_version,
        f.seq AS fielding_seq, f.catch_fielder_id, f.catch_fielder_position,
        f.interception_fielder_position, f.selection_path, f.catch_type, f.error_type,
        f.catch_time_sec, f.catch_z_m, f.waiting_time_sec, f.play_type
 FROM player_game_batting b
 LEFT JOIN player_game_fielding f
   ON f.game_id=b.game_id AND f.count_seq=b.count_seq AND f.seq=1
 WHERE b.game_id BETWEEN :first_game_id AND :last_game_id
   AND b.adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
), directions AS (
 SELECT *,
   CASE WHEN final_hla_deg IS NULL THEN 'unknown'
        WHEN ABS(final_hla_deg)>=90 THEN 'backward'
        WHEN ABS(final_hla_deg)>=45 THEN 'foul_side' ELSE 'fair' END AS launch_direction,
   CASE WHEN first_bounce_angle IS NULL THEN 'unknown'
        WHEN ABS(first_bounce_angle)>=90 THEN 'backward'
        WHEN ABS(first_bounce_angle)>=45 THEN 'foul_side' ELSE 'fair' END AS bounce_direction,
   CASE WHEN polar_angle IS NULL THEN 'unknown'
        WHEN ABS(polar_angle)>=90 THEN 'backward'
        WHEN ABS(polar_angle)>=45 THEN 'foul_side' ELSE 'fair' END AS final_direction,
   CASE WHEN fielding_seq IS NULL THEN 'missing_row'
        WHEN catch_type IS NULL THEN 'missing_catch_diagnostics' ELSE 'recorded' END AS fielding_coverage,
   CASE WHEN outbound_result='Foul' THEN 'outbound_foul'
        WHEN outbound_result='HomeRun' THEN 'outbound_home_run'
        WHEN outbound_result='GroundRuleDouble' THEN 'outbound_ground_rule_double'
        WHEN outbound_result IS NULL OR outbound_result<>'InField' THEN 'unknown_outbound'
        WHEN catch_type='DirectFly' THEN 'direct_fly_before_final_foul_check'
        WHEN ABS(polar_angle)>=45 THEN 'final_position_foul_check'
        ELSE 'fair_or_missing_fly_diagnostics' END AS resolution_hint,
   CASE WHEN catch_type<>'DirectFly' OR catch_type IS NULL THEN 'not_direct_fly'
        WHEN catch_time_sec IS NULL THEN 'missing_catch_time'
        WHEN (first_bounce_time IS NOT NULL AND catch_time_sec>=first_bounce_time-1e-9)
          OR (fence_impact_time IS NOT NULL AND catch_time_sec>=fence_impact_time-1e-9)
          THEN 'at_or_after_event_boundary'
        WHEN first_bounce_time IS NULL AND fence_impact_time IS NULL THEN 'no_recorded_boundary'
        ELSE 'before_recorded_boundaries' END AS fly_boundary_status
 FROM contacts
), diagnosed AS (
 SELECT *,
   CASE WHEN launch_direction='unknown' OR final_direction='unknown' THEN 'unknown_direction'
        WHEN launch_direction IN ('foul_side','backward') AND result<>'Foul' THEN 'launch_foul_non_foul_result'
        WHEN launch_direction='fair' AND result='Foul' THEN 'launch_fair_foul_result'
        ELSE 'direction_result_agree' END AS discrepancy
 FROM directions
)
SELECT game_id,count_seq,batter_id,hla_version,adjusted_contact_type,
 launch_speed AS ev_ms,launch_angle AS vla_deg,final_hla_deg,
 launch_direction,first_bounce_angle,bounce_direction,polar_angle AS final_angle,final_direction,
 polar_distance AS final_distance_m,total_time,first_bounce_time,fence_impact_time,
 outbound_result,result,fielder_position AS batting_fielder_position,fielding_coverage,
 catch_fielder_id,catch_fielder_position,interception_fielder_position,selection_path,
 catch_type,error_type,catch_time_sec,catch_z_m,waiting_time_sec,play_type,
 discrepancy,resolution_hint,fly_boundary_status
FROM diagnosed WHERE discrepancy<>'direction_result_agree'
ORDER BY game_id,count_seq LIMIT 200;
