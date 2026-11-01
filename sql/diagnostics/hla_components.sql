-- Bind one code-version range. All diagnostics are read-only and reuse saved draws.

-- 1. Coverage and independent reconstruction (expect zero mismatches)
WITH contacts AS (
 SELECT * FROM player_game_batting
 WHERE game_id BETWEEN :first_game_id AND :last_game_id
 AND adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
), components AS (
 SELECT *, hla_handedness_sign*0.85*approach_variation_deg AS approach_component,
   hla_handedness_sign*0.85*pull_bias_deg AS pull_component,
   hla_handedness_sign*0.85*timing_angle_error AS timing_component,
   hla_handedness_sign*0.25*ASIN(MAX(-1.0,MIN(1.0,contact_offset_x_m/0.070)))*180.0/ACOS(-1.0) AS rebound_component,
   hla_handedness_sign*0.85*angular_velocity*adapted_timing_offset_sec*180.0/ACOS(-1.0) AS original_timing_component
 FROM contacts WHERE hla_handedness_sign IN (-1,1) AND contact_offset_x_m IS NOT NULL
 AND adapted_timing_offset_sec IS NOT NULL
), reconstructed AS (
 SELECT *,approach_component+pull_component+timing_component+rebound_component AS replay_hla,
   approach_component+pull_component+original_timing_component+rebound_component AS original_timing_hla
 FROM components
)
SELECT (SELECT COUNT(*) FROM contacts) AS contacts,COUNT(*) AS comparable_contacts,
 (SELECT COUNT(*) FROM contacts)-COUNT(*) AS missing_inputs,
 COALESCE(SUM(CASE WHEN ABS(replay_hla-final_hla_deg)>1e-8 THEN 1 ELSE 0 END),0) AS hla_mismatches
FROM reconstructed;

-- 2. Signed and absolute component means by contact type and actual orientation
WITH contacts AS (
 SELECT * FROM player_game_batting
 WHERE game_id BETWEEN :first_game_id AND :last_game_id
 AND adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
), components AS (
 SELECT *, hla_handedness_sign*0.85*approach_variation_deg AS approach_component,
   hla_handedness_sign*0.85*pull_bias_deg AS pull_component,
   hla_handedness_sign*0.85*timing_angle_error AS timing_component,
   hla_handedness_sign*0.25*ASIN(MAX(-1.0,MIN(1.0,contact_offset_x_m/0.070)))*180.0/ACOS(-1.0) AS rebound_component,
   hla_handedness_sign*0.85*angular_velocity*adapted_timing_offset_sec*180.0/ACOS(-1.0) AS original_timing_component
 FROM contacts WHERE hla_handedness_sign IN (-1,1) AND contact_offset_x_m IS NOT NULL
 AND adapted_timing_offset_sec IS NOT NULL
), reconstructed AS (
 SELECT *,approach_component+pull_component+timing_component+rebound_component AS replay_hla,
   approach_component+pull_component+original_timing_component+rebound_component AS original_timing_hla
 FROM components
)
SELECT adjusted_contact_type,hla_handedness_sign,COUNT(*) AS contacts,
 ROUND(AVG(ABS(approach_component)),3) AS avg_abs_approach_deg,
 ROUND(AVG(ABS(pull_component)),3) AS avg_abs_pull_deg,
 ROUND(AVG(ABS(timing_component)),3) AS avg_abs_timing_deg,
 ROUND(AVG(ABS(original_timing_component)),3) AS avg_abs_original_timing_deg,
 ROUND(AVG(ABS(rebound_component)),3) AS avg_abs_rebound_deg,
 ROUND(AVG(timing_component),3) AS avg_signed_timing_deg,
 ROUND(AVG(rebound_component),3) AS avg_signed_rebound_deg,
 SUM(CASE WHEN ABS(contact_offset_x_m)>=0.070 THEN 1 ELSE 0 END) AS rebound_clamped_contacts,
 SUM(CASE WHEN ABS(final_hla_deg)>=45 THEN 1 ELSE 0 END) AS launch_foul_directions,
 SUM(CASE WHEN result='Foul' THEN 1 ELSE 0 END) AS result_fouls,
 SUM(CASE WHEN ABS(original_timing_hla)>=45 THEN 1 ELSE 0 END) AS original_timing_foul_directions
FROM reconstructed GROUP BY adjusted_contact_type,hla_handedness_sign
ORDER BY adjusted_contact_type,hla_handedness_sign;

-- 3. Current vs original-timing HLA distributions on identical contacts
WITH contacts AS (
 SELECT * FROM player_game_batting
 WHERE game_id BETWEEN :first_game_id AND :last_game_id
 AND adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
), components AS (
 SELECT *, hla_handedness_sign*0.85*approach_variation_deg AS approach_component,
   hla_handedness_sign*0.85*pull_bias_deg AS pull_component,
   hla_handedness_sign*0.85*timing_angle_error AS timing_component,
   hla_handedness_sign*0.25*ASIN(MAX(-1.0,MIN(1.0,contact_offset_x_m/0.070)))*180.0/ACOS(-1.0) AS rebound_component,
   hla_handedness_sign*0.85*angular_velocity*adapted_timing_offset_sec*180.0/ACOS(-1.0) AS original_timing_component
 FROM contacts WHERE hla_handedness_sign IN (-1,1) AND contact_offset_x_m IS NOT NULL
 AND adapted_timing_offset_sec IS NOT NULL
), reconstructed AS (
 SELECT *,approach_component+pull_component+timing_component+rebound_component AS replay_hla,
   approach_component+pull_component+original_timing_component+rebound_component AS original_timing_hla
 FROM components
)
, variants AS (
 SELECT adjusted_contact_type,'current' AS variant,final_hla_deg AS hla FROM reconstructed
 UNION ALL
 SELECT adjusted_contact_type,'original_timing',original_timing_hla FROM reconstructed
)
SELECT adjusted_contact_type,variant,
 CASE WHEN hla< -45 THEN '<-45' WHEN hla< -30 THEN '-45..<-30'
      WHEN hla< -15 THEN '-30..<-15' WHEN hla<0 THEN '-15..<0'
      WHEN hla<15 THEN '0..<15' WHEN hla<30 THEN '15..<30'
      WHEN hla<45 THEN '30..<45' ELSE '45+' END AS hla_band,
 COUNT(*) AS contacts,
 ROUND(100.0*COUNT(*)/SUM(COUNT(*)) OVER(PARTITION BY adjusted_contact_type,variant),2) AS pct_within_type
FROM variants GROUP BY adjusted_contact_type,variant,hla_band
ORDER BY adjusted_contact_type,variant,hla_band;

-- 4. Marginal timing-only transitions; launch directions, not replayed game results
WITH contacts AS (
 SELECT * FROM player_game_batting
 WHERE game_id BETWEEN :first_game_id AND :last_game_id
 AND adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
), components AS (
 SELECT *, hla_handedness_sign*0.85*approach_variation_deg AS approach_component,
   hla_handedness_sign*0.85*pull_bias_deg AS pull_component,
   hla_handedness_sign*0.85*timing_angle_error AS timing_component,
   hla_handedness_sign*0.25*ASIN(MAX(-1.0,MIN(1.0,contact_offset_x_m/0.070)))*180.0/ACOS(-1.0) AS rebound_component,
   hla_handedness_sign*0.85*angular_velocity*adapted_timing_offset_sec*180.0/ACOS(-1.0) AS original_timing_component
 FROM contacts WHERE hla_handedness_sign IN (-1,1) AND contact_offset_x_m IS NOT NULL
 AND adapted_timing_offset_sec IS NOT NULL
), reconstructed AS (
 SELECT *,approach_component+pull_component+timing_component+rebound_component AS replay_hla,
   approach_component+pull_component+original_timing_component+rebound_component AS original_timing_hla
 FROM components
)
SELECT CASE WHEN ABS(final_hla_deg)>=45 THEN 'foul_direction' ELSE 'fair_direction' END AS current_direction,
 CASE WHEN ABS(original_timing_hla)>=45 THEN 'foul_direction' ELSE 'fair_direction' END AS original_timing_direction,
 COUNT(*) AS contacts,
 ROUND(AVG(ABS(timing_component-original_timing_component)),3) AS avg_abs_timing_component_change_deg
FROM reconstructed WHERE adjusted_contact_type='MarginalContact'
GROUP BY current_direction,original_timing_direction;
