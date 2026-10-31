-- Bind a single code-version game-ID range. Read-only, no random sampling.

-- 1. Coverage and baseline replay: check these before interpreting sensitivity
WITH selected AS (
 SELECT * FROM player_game_batting
 WHERE game_id BETWEEN :first_game_id AND :last_game_id
   AND adjusted_contact_type IN ('SwungAndMiss','SolidContact','WeakContact','MarginalContact')
), valid AS (
 SELECT *, CASE WHEN decision_count_status IN ('C02','C12','C22','C32') THEN 'two_strikes'
                WHEN decision_count_status IS NULL THEN 'unknown_count' ELSE 'before_two_strikes' END AS count_group,
   CASE WHEN decision_location_x IS NULL OR decision_location_y IS NULL THEN 'unknown_zone'
        WHEN ABS(decision_location_x)<=1 AND ABS(decision_location_y)<=1 THEN 'in_zone' ELSE 'outside_zone' END AS zone_group,
   CASE WHEN actual_bat_angle_deg<20 THEN '<20deg' WHEN actual_bat_angle_deg<40 THEN '20..<40deg'
        ELSE '40deg+' END AS angle_band,
   CASE WHEN adapted_timing_offset_sec IS NULL THEN 'unknown_timing'
        WHEN ABS(adapted_timing_offset_sec)<0.002 THEN '<2ms'
        WHEN ABS(adapted_timing_offset_sec)<0.005 THEN '2..<5ms'
        WHEN ABS(adapted_timing_offset_sec)<0.010 THEN '5..<10ms'
        WHEN ABS(adapted_timing_offset_sec)<0.015 THEN '10..<15ms' ELSE '15ms+' END AS timing_band
 FROM selected WHERE actual_bat_angle_deg IS NOT NULL AND contact_offset_x_m IS NOT NULL
   AND contact_offset_z_m IS NOT NULL AND timing_impact_x_m IS NOT NULL
   AND length_offset_m IS NOT NULL AND thickness_offset_m IS NOT NULL
), scales(timing_scale) AS (VALUES (1.0),(0.75),(0.5),(0.0)), offsets AS (
 SELECT v.*, s.timing_scale,
   contact_offset_x_m + (s.timing_scale-1.0)*timing_impact_x_m AS probe_x,
   contact_offset_z_m AS probe_z, actual_bat_angle_deg*ACOS(-1.0)/180.0 AS angle_rad
 FROM valid v CROSS JOIN scales s
), geometry AS (
 SELECT *, ABS(probe_x*COS(angle_rad)+probe_z*SIN(angle_rad)) AS probe_length_m,
   ABS(-probe_x*SIN(angle_rad)+probe_z*COS(angle_rad)) AS probe_thickness_m
 FROM offsets
), classified AS (
 SELECT *, CASE WHEN probe_length_m>0.16 AND probe_thickness_m>0.070 THEN 'both_exceeded'
                WHEN probe_length_m>0.16 THEN 'length_only'
                WHEN probe_thickness_m>0.070 THEN 'thickness_only' ELSE 'within_limits' END AS probe_group
 FROM geometry
), baseline AS (
 SELECT game_id,count_seq,probe_group AS baseline_group,
   CASE WHEN ABS(probe_length_m-length_offset_m)>1e-9 OR ABS(probe_thickness_m-thickness_offset_m)>1e-9
          OR ((probe_group='within_limits') <> (adjusted_contact_type<>'SwungAndMiss'))
        THEN 1 ELSE 0 END AS baseline_mismatch
 FROM classified WHERE timing_scale=1.0
), compared AS (
 SELECT c.*, b.baseline_group,b.baseline_mismatch
 FROM classified c JOIN baseline b USING(game_id,count_seq)
)
SELECT (SELECT COUNT(*) FROM selected) AS recorded_swings,
 (SELECT COUNT(*) FROM valid) AS comparable_swings,
 (SELECT COUNT(*) FROM selected)-(SELECT COUNT(*) FROM valid) AS missing_input_swings,
 (SELECT COUNT(*) FROM selected WHERE actual_bat_angle_deg IS NULL) AS missing_angle_swings,
 (SELECT COUNT(*) FROM valid WHERE adapted_timing_offset_sec IS NULL) AS missing_timing_band_swings,
 COALESCE(SUM(baseline_mismatch),0) AS baseline_mismatches
FROM baseline;

-- 2. Overall sensitivity on the identical set of swings
WITH selected AS (
 SELECT * FROM player_game_batting
 WHERE game_id BETWEEN :first_game_id AND :last_game_id
   AND adjusted_contact_type IN ('SwungAndMiss','SolidContact','WeakContact','MarginalContact')
), valid AS (
 SELECT *, CASE WHEN decision_count_status IN ('C02','C12','C22','C32') THEN 'two_strikes'
                WHEN decision_count_status IS NULL THEN 'unknown_count' ELSE 'before_two_strikes' END AS count_group,
   CASE WHEN decision_location_x IS NULL OR decision_location_y IS NULL THEN 'unknown_zone'
        WHEN ABS(decision_location_x)<=1 AND ABS(decision_location_y)<=1 THEN 'in_zone' ELSE 'outside_zone' END AS zone_group,
   CASE WHEN actual_bat_angle_deg<20 THEN '<20deg' WHEN actual_bat_angle_deg<40 THEN '20..<40deg'
        ELSE '40deg+' END AS angle_band,
   CASE WHEN adapted_timing_offset_sec IS NULL THEN 'unknown_timing'
        WHEN ABS(adapted_timing_offset_sec)<0.002 THEN '<2ms'
        WHEN ABS(adapted_timing_offset_sec)<0.005 THEN '2..<5ms'
        WHEN ABS(adapted_timing_offset_sec)<0.010 THEN '5..<10ms'
        WHEN ABS(adapted_timing_offset_sec)<0.015 THEN '10..<15ms' ELSE '15ms+' END AS timing_band
 FROM selected WHERE actual_bat_angle_deg IS NOT NULL AND contact_offset_x_m IS NOT NULL
   AND contact_offset_z_m IS NOT NULL AND timing_impact_x_m IS NOT NULL
   AND length_offset_m IS NOT NULL AND thickness_offset_m IS NOT NULL
), scales(timing_scale) AS (VALUES (1.0),(0.75),(0.5),(0.0)), offsets AS (
 SELECT v.*, s.timing_scale,
   contact_offset_x_m + (s.timing_scale-1.0)*timing_impact_x_m AS probe_x,
   contact_offset_z_m AS probe_z, actual_bat_angle_deg*ACOS(-1.0)/180.0 AS angle_rad
 FROM valid v CROSS JOIN scales s
), geometry AS (
 SELECT *, ABS(probe_x*COS(angle_rad)+probe_z*SIN(angle_rad)) AS probe_length_m,
   ABS(-probe_x*SIN(angle_rad)+probe_z*COS(angle_rad)) AS probe_thickness_m
 FROM offsets
), classified AS (
 SELECT *, CASE WHEN probe_length_m>0.16 AND probe_thickness_m>0.070 THEN 'both_exceeded'
                WHEN probe_length_m>0.16 THEN 'length_only'
                WHEN probe_thickness_m>0.070 THEN 'thickness_only' ELSE 'within_limits' END AS probe_group
 FROM geometry
), baseline AS (
 SELECT game_id,count_seq,probe_group AS baseline_group,
   CASE WHEN ABS(probe_length_m-length_offset_m)>1e-9 OR ABS(probe_thickness_m-thickness_offset_m)>1e-9
          OR ((probe_group='within_limits') <> (adjusted_contact_type<>'SwungAndMiss'))
        THEN 1 ELSE 0 END AS baseline_mismatch
 FROM classified WHERE timing_scale=1.0
), compared AS (
 SELECT c.*, b.baseline_group,b.baseline_mismatch
 FROM classified c JOIN baseline b USING(game_id,count_seq)
)
SELECT timing_scale, COUNT(*) AS swings,
 SUM(CASE WHEN probe_group='within_limits' THEN 1 ELSE 0 END) AS possible_contacts,
 ROUND(100.0*AVG(CASE WHEN probe_group='within_limits' THEN 1 ELSE 0 END),2) AS contact_pct,
 SUM(CASE WHEN baseline_group<>'within_limits' AND probe_group='within_limits' THEN 1 ELSE 0 END) AS recovered_contacts,
 SUM(CASE WHEN baseline_group='within_limits' AND probe_group<>'within_limits' THEN 1 ELSE 0 END) AS lost_contacts,
 SUM(baseline_mismatch) AS baseline_mismatches
FROM compared GROUP BY timing_scale ORDER BY timing_scale DESC;

-- 3. Sensitivity by count, zone, original timing band and fixed bat angle
WITH selected AS (
 SELECT * FROM player_game_batting
 WHERE game_id BETWEEN :first_game_id AND :last_game_id
   AND adjusted_contact_type IN ('SwungAndMiss','SolidContact','WeakContact','MarginalContact')
), valid AS (
 SELECT *, CASE WHEN decision_count_status IN ('C02','C12','C22','C32') THEN 'two_strikes'
                WHEN decision_count_status IS NULL THEN 'unknown_count' ELSE 'before_two_strikes' END AS count_group,
   CASE WHEN decision_location_x IS NULL OR decision_location_y IS NULL THEN 'unknown_zone'
        WHEN ABS(decision_location_x)<=1 AND ABS(decision_location_y)<=1 THEN 'in_zone' ELSE 'outside_zone' END AS zone_group,
   CASE WHEN actual_bat_angle_deg<20 THEN '<20deg' WHEN actual_bat_angle_deg<40 THEN '20..<40deg'
        ELSE '40deg+' END AS angle_band,
   CASE WHEN adapted_timing_offset_sec IS NULL THEN 'unknown_timing'
        WHEN ABS(adapted_timing_offset_sec)<0.002 THEN '<2ms'
        WHEN ABS(adapted_timing_offset_sec)<0.005 THEN '2..<5ms'
        WHEN ABS(adapted_timing_offset_sec)<0.010 THEN '5..<10ms'
        WHEN ABS(adapted_timing_offset_sec)<0.015 THEN '10..<15ms' ELSE '15ms+' END AS timing_band
 FROM selected WHERE actual_bat_angle_deg IS NOT NULL AND contact_offset_x_m IS NOT NULL
   AND contact_offset_z_m IS NOT NULL AND timing_impact_x_m IS NOT NULL
   AND length_offset_m IS NOT NULL AND thickness_offset_m IS NOT NULL
), scales(timing_scale) AS (VALUES (1.0),(0.75),(0.5),(0.0)), offsets AS (
 SELECT v.*, s.timing_scale,
   contact_offset_x_m + (s.timing_scale-1.0)*timing_impact_x_m AS probe_x,
   contact_offset_z_m AS probe_z, actual_bat_angle_deg*ACOS(-1.0)/180.0 AS angle_rad
 FROM valid v CROSS JOIN scales s
), geometry AS (
 SELECT *, ABS(probe_x*COS(angle_rad)+probe_z*SIN(angle_rad)) AS probe_length_m,
   ABS(-probe_x*SIN(angle_rad)+probe_z*COS(angle_rad)) AS probe_thickness_m
 FROM offsets
), classified AS (
 SELECT *, CASE WHEN probe_length_m>0.16 AND probe_thickness_m>0.070 THEN 'both_exceeded'
                WHEN probe_length_m>0.16 THEN 'length_only'
                WHEN probe_thickness_m>0.070 THEN 'thickness_only' ELSE 'within_limits' END AS probe_group
 FROM geometry
), baseline AS (
 SELECT game_id,count_seq,probe_group AS baseline_group,
   CASE WHEN ABS(probe_length_m-length_offset_m)>1e-9 OR ABS(probe_thickness_m-thickness_offset_m)>1e-9
          OR ((probe_group='within_limits') <> (adjusted_contact_type<>'SwungAndMiss'))
        THEN 1 ELSE 0 END AS baseline_mismatch
 FROM classified WHERE timing_scale=1.0
), compared AS (
 SELECT c.*, b.baseline_group,b.baseline_mismatch
 FROM classified c JOIN baseline b USING(game_id,count_seq)
)
SELECT count_group,zone_group,timing_band,angle_band,timing_scale,COUNT(*) AS swings,
 SUM(CASE WHEN probe_group='within_limits' THEN 1 ELSE 0 END) AS possible_contacts,
 ROUND(100.0*AVG(CASE WHEN probe_group='within_limits' THEN 1 ELSE 0 END),2) AS contact_pct,
 ROUND(AVG(probe_length_m)*100,3) AS avg_length_cm,
 ROUND(AVG(probe_thickness_m)*100,3) AS avg_thickness_cm,
 SUM(baseline_mismatch) AS baseline_mismatches
FROM compared GROUP BY count_group,zone_group,timing_band,angle_band,timing_scale
ORDER BY count_group,zone_group,timing_band,angle_band,timing_scale DESC;

-- 4. Classification transitions (includes contacts lost through cancellation changes)
WITH selected AS (
 SELECT * FROM player_game_batting
 WHERE game_id BETWEEN :first_game_id AND :last_game_id
   AND adjusted_contact_type IN ('SwungAndMiss','SolidContact','WeakContact','MarginalContact')
), valid AS (
 SELECT *, CASE WHEN decision_count_status IN ('C02','C12','C22','C32') THEN 'two_strikes'
                WHEN decision_count_status IS NULL THEN 'unknown_count' ELSE 'before_two_strikes' END AS count_group,
   CASE WHEN decision_location_x IS NULL OR decision_location_y IS NULL THEN 'unknown_zone'
        WHEN ABS(decision_location_x)<=1 AND ABS(decision_location_y)<=1 THEN 'in_zone' ELSE 'outside_zone' END AS zone_group,
   CASE WHEN actual_bat_angle_deg<20 THEN '<20deg' WHEN actual_bat_angle_deg<40 THEN '20..<40deg'
        ELSE '40deg+' END AS angle_band,
   CASE WHEN adapted_timing_offset_sec IS NULL THEN 'unknown_timing'
        WHEN ABS(adapted_timing_offset_sec)<0.002 THEN '<2ms'
        WHEN ABS(adapted_timing_offset_sec)<0.005 THEN '2..<5ms'
        WHEN ABS(adapted_timing_offset_sec)<0.010 THEN '5..<10ms'
        WHEN ABS(adapted_timing_offset_sec)<0.015 THEN '10..<15ms' ELSE '15ms+' END AS timing_band
 FROM selected WHERE actual_bat_angle_deg IS NOT NULL AND contact_offset_x_m IS NOT NULL
   AND contact_offset_z_m IS NOT NULL AND timing_impact_x_m IS NOT NULL
   AND length_offset_m IS NOT NULL AND thickness_offset_m IS NOT NULL
), scales(timing_scale) AS (VALUES (1.0),(0.75),(0.5),(0.0)), offsets AS (
 SELECT v.*, s.timing_scale,
   contact_offset_x_m + (s.timing_scale-1.0)*timing_impact_x_m AS probe_x,
   contact_offset_z_m AS probe_z, actual_bat_angle_deg*ACOS(-1.0)/180.0 AS angle_rad
 FROM valid v CROSS JOIN scales s
), geometry AS (
 SELECT *, ABS(probe_x*COS(angle_rad)+probe_z*SIN(angle_rad)) AS probe_length_m,
   ABS(-probe_x*SIN(angle_rad)+probe_z*COS(angle_rad)) AS probe_thickness_m
 FROM offsets
), classified AS (
 SELECT *, CASE WHEN probe_length_m>0.16 AND probe_thickness_m>0.070 THEN 'both_exceeded'
                WHEN probe_length_m>0.16 THEN 'length_only'
                WHEN probe_thickness_m>0.070 THEN 'thickness_only' ELSE 'within_limits' END AS probe_group
 FROM geometry
), baseline AS (
 SELECT game_id,count_seq,probe_group AS baseline_group,
   CASE WHEN ABS(probe_length_m-length_offset_m)>1e-9 OR ABS(probe_thickness_m-thickness_offset_m)>1e-9
          OR ((probe_group='within_limits') <> (adjusted_contact_type<>'SwungAndMiss'))
        THEN 1 ELSE 0 END AS baseline_mismatch
 FROM classified WHERE timing_scale=1.0
), compared AS (
 SELECT c.*, b.baseline_group,b.baseline_mismatch
 FROM classified c JOIN baseline b USING(game_id,count_seq)
)
SELECT timing_scale,baseline_group,probe_group,COUNT(*) AS swings
FROM compared GROUP BY timing_scale,baseline_group,probe_group
ORDER BY timing_scale DESC,baseline_group,probe_group;
