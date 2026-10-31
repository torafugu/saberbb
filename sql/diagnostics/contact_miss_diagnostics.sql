-- Bind first_game_id and last_game_id to one code version. Strict > matches current contact checks.

-- 1. Coverage and consistency (all inferred or explicitly recorded swings)
WITH config(first_game_id,last_game_id,length_limit_m,thickness_limit_m) AS (
    VALUES (:first_game_id,:last_game_id,0.16,0.070)
), swings AS (
    SELECT b.*, c.length_limit_m, c.thickness_limit_m
    FROM player_game_batting b CROSS JOIN config c
    WHERE b.game_id BETWEEN c.first_game_id AND c.last_game_id
      AND (b.decision_swung=1 OR b.adjusted_contact_type IN
        ('SwungAndMiss','SolidContact','WeakContact','MarginalContact'))
), classified AS (
    SELECT *,
        CASE WHEN length_offset_m IS NULL OR thickness_offset_m IS NULL
                       OR length_offset_m<0 OR thickness_offset_m<0 THEN 'unknown_geometry'
             WHEN length_offset_m>length_limit_m AND thickness_offset_m>thickness_limit_m THEN 'both_exceeded'
             WHEN length_offset_m>length_limit_m THEN 'length_only'
             WHEN thickness_offset_m>thickness_limit_m THEN 'thickness_only'
             ELSE 'within_limits' END AS geometry_group,
        CASE WHEN decision_count_status IN ('C02','C12','C22','C32') THEN 'two_strikes'
             WHEN decision_count_status IN ('C00','C01','C10','C11','C20','C21','C30','C31') THEN 'before_two_strikes'
             ELSE 'unknown_count' END AS count_group,
        CASE WHEN decision_location_x IS NULL OR decision_location_y IS NULL THEN 'unknown_zone'
             WHEN ABS(decision_location_x)<=1 AND ABS(decision_location_y)<=1 THEN 'in_zone'
             ELSE 'outside_zone' END AS zone_group,
        CASE WHEN adapted_timing_offset_sec IS NULL THEN 'unknown_timing'
             WHEN ABS(adapted_timing_offset_sec)<0.002 THEN '00_<2ms'
             WHEN ABS(adapted_timing_offset_sec)<0.005 THEN '01_2..<5ms'
             WHEN ABS(adapted_timing_offset_sec)<0.010 THEN '02_5..<10ms'
             WHEN ABS(adapted_timing_offset_sec)<0.015 THEN '03_10..<15ms'
             ELSE '04_15ms+' END AS timing_band
    FROM swings
), diagnosed AS (
    SELECT *, CASE
        WHEN geometry_group='unknown_geometry' THEN 'unknown'
        WHEN adjusted_contact_type='SwungAndMiss' AND geometry_group='within_limits' THEN 'miss_within_limits'
        WHEN adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
             AND geometry_group<>'within_limits' THEN 'contact_outside_limits'
        WHEN adjusted_contact_type NOT IN ('SwungAndMiss','SolidContact','WeakContact','MarginalContact') THEN 'unknown_contact_type'
        ELSE 'consistent' END AS consistency
    FROM classified
)
SELECT count_group, zone_group, consistency, COUNT(*) AS swings,
 SUM(CASE WHEN decision_swung=1 THEN 1 ELSE 0 END) AS explicit_swing_records,
 SUM(CASE WHEN adapted_timing_offset_sec IS NULL THEN 1 ELSE 0 END) AS missing_timing_records
FROM diagnosed GROUP BY count_group,zone_group,consistency
ORDER BY count_group,zone_group,consistency;

-- 2. Overall mutually exclusive geometry groups, including contact as a reference
WITH config(first_game_id,last_game_id,length_limit_m,thickness_limit_m) AS (
    VALUES (:first_game_id,:last_game_id,0.16,0.070)
), swings AS (
    SELECT b.*, c.length_limit_m, c.thickness_limit_m
    FROM player_game_batting b CROSS JOIN config c
    WHERE b.game_id BETWEEN c.first_game_id AND c.last_game_id
      AND (b.decision_swung=1 OR b.adjusted_contact_type IN
        ('SwungAndMiss','SolidContact','WeakContact','MarginalContact'))
), classified AS (
    SELECT *,
        CASE WHEN length_offset_m IS NULL OR thickness_offset_m IS NULL
                       OR length_offset_m<0 OR thickness_offset_m<0 THEN 'unknown_geometry'
             WHEN length_offset_m>length_limit_m AND thickness_offset_m>thickness_limit_m THEN 'both_exceeded'
             WHEN length_offset_m>length_limit_m THEN 'length_only'
             WHEN thickness_offset_m>thickness_limit_m THEN 'thickness_only'
             ELSE 'within_limits' END AS geometry_group,
        CASE WHEN decision_count_status IN ('C02','C12','C22','C32') THEN 'two_strikes'
             WHEN decision_count_status IN ('C00','C01','C10','C11','C20','C21','C30','C31') THEN 'before_two_strikes'
             ELSE 'unknown_count' END AS count_group,
        CASE WHEN decision_location_x IS NULL OR decision_location_y IS NULL THEN 'unknown_zone'
             WHEN ABS(decision_location_x)<=1 AND ABS(decision_location_y)<=1 THEN 'in_zone'
             ELSE 'outside_zone' END AS zone_group,
        CASE WHEN adapted_timing_offset_sec IS NULL THEN 'unknown_timing'
             WHEN ABS(adapted_timing_offset_sec)<0.002 THEN '00_<2ms'
             WHEN ABS(adapted_timing_offset_sec)<0.005 THEN '01_2..<5ms'
             WHEN ABS(adapted_timing_offset_sec)<0.010 THEN '02_5..<10ms'
             WHEN ABS(adapted_timing_offset_sec)<0.015 THEN '03_10..<15ms'
             ELSE '04_15ms+' END AS timing_band
    FROM swings
), diagnosed AS (
    SELECT *, CASE
        WHEN geometry_group='unknown_geometry' THEN 'unknown'
        WHEN adjusted_contact_type='SwungAndMiss' AND geometry_group='within_limits' THEN 'miss_within_limits'
        WHEN adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
             AND geometry_group<>'within_limits' THEN 'contact_outside_limits'
        WHEN adjusted_contact_type NOT IN ('SwungAndMiss','SolidContact','WeakContact','MarginalContact') THEN 'unknown_contact_type'
        ELSE 'consistent' END AS consistency
    FROM classified
)
SELECT geometry_group, COUNT(*) AS swings,
 SUM(CASE WHEN adjusted_contact_type='SwungAndMiss' THEN 1 ELSE 0 END) AS whiffs,
 ROUND(100.0*COUNT(*)/SUM(COUNT(*)) OVER (),2) AS pct_of_swings,
 ROUND(AVG(ABS(adapted_timing_offset_sec))*1000,3) AS avg_abs_timing_ms,
 ROUND(AVG(ABS(timing_impact_x_m))*100,3) AS avg_abs_timing_impact_cm,
 ROUND(AVG(ABS(adapted_horizontal_offset_m))*100,3) AS avg_abs_horizontal_cm,
 ROUND(AVG(ABS(adapted_vertical_offset_m))*100,3) AS avg_abs_vertical_cm
FROM diagnosed GROUP BY geometry_group ORDER BY geometry_group;

-- 3. Geometry by count, zone and adapted timing; shares use all swings in each stratum
WITH config(first_game_id,last_game_id,length_limit_m,thickness_limit_m) AS (
    VALUES (:first_game_id,:last_game_id,0.16,0.070)
), swings AS (
    SELECT b.*, c.length_limit_m, c.thickness_limit_m
    FROM player_game_batting b CROSS JOIN config c
    WHERE b.game_id BETWEEN c.first_game_id AND c.last_game_id
      AND (b.decision_swung=1 OR b.adjusted_contact_type IN
        ('SwungAndMiss','SolidContact','WeakContact','MarginalContact'))
), classified AS (
    SELECT *,
        CASE WHEN length_offset_m IS NULL OR thickness_offset_m IS NULL
                       OR length_offset_m<0 OR thickness_offset_m<0 THEN 'unknown_geometry'
             WHEN length_offset_m>length_limit_m AND thickness_offset_m>thickness_limit_m THEN 'both_exceeded'
             WHEN length_offset_m>length_limit_m THEN 'length_only'
             WHEN thickness_offset_m>thickness_limit_m THEN 'thickness_only'
             ELSE 'within_limits' END AS geometry_group,
        CASE WHEN decision_count_status IN ('C02','C12','C22','C32') THEN 'two_strikes'
             WHEN decision_count_status IN ('C00','C01','C10','C11','C20','C21','C30','C31') THEN 'before_two_strikes'
             ELSE 'unknown_count' END AS count_group,
        CASE WHEN decision_location_x IS NULL OR decision_location_y IS NULL THEN 'unknown_zone'
             WHEN ABS(decision_location_x)<=1 AND ABS(decision_location_y)<=1 THEN 'in_zone'
             ELSE 'outside_zone' END AS zone_group,
        CASE WHEN adapted_timing_offset_sec IS NULL THEN 'unknown_timing'
             WHEN ABS(adapted_timing_offset_sec)<0.002 THEN '00_<2ms'
             WHEN ABS(adapted_timing_offset_sec)<0.005 THEN '01_2..<5ms'
             WHEN ABS(adapted_timing_offset_sec)<0.010 THEN '02_5..<10ms'
             WHEN ABS(adapted_timing_offset_sec)<0.015 THEN '03_10..<15ms'
             ELSE '04_15ms+' END AS timing_band
    FROM swings
), diagnosed AS (
    SELECT *, CASE
        WHEN geometry_group='unknown_geometry' THEN 'unknown'
        WHEN adjusted_contact_type='SwungAndMiss' AND geometry_group='within_limits' THEN 'miss_within_limits'
        WHEN adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
             AND geometry_group<>'within_limits' THEN 'contact_outside_limits'
        WHEN adjusted_contact_type NOT IN ('SwungAndMiss','SolidContact','WeakContact','MarginalContact') THEN 'unknown_contact_type'
        ELSE 'consistent' END AS consistency
    FROM classified
)
SELECT count_group,zone_group,timing_band,geometry_group,COUNT(*) AS swings,
 SUM(CASE WHEN adjusted_contact_type='SwungAndMiss' THEN 1 ELSE 0 END) AS whiffs,
 ROUND(100.0*COUNT(*)/SUM(COUNT(*)) OVER (PARTITION BY count_group,zone_group,timing_band),2) AS pct_of_stratum_swings,
 ROUND(AVG(length_offset_m)*100,3) AS avg_length_cm,
 ROUND(AVG(thickness_offset_m)*100,3) AS avg_thickness_cm,
 COUNT(adapted_timing_offset_sec) AS timing_records,
 COUNT(timing_impact_x_m) AS timing_impact_records,
 ROUND(AVG(ABS(timing_impact_x_m))*100,3) AS avg_abs_timing_impact_cm
FROM diagnosed GROUP BY count_group,zone_group,timing_band,geometry_group
ORDER BY count_group,zone_group,timing_band,geometry_group;

-- 4. Whiff detail for auditing both-exceeded vs length-only; no hidden LIMIT
WITH config(first_game_id,last_game_id,length_limit_m,thickness_limit_m) AS (
    VALUES (:first_game_id,:last_game_id,0.16,0.070)
), swings AS (
    SELECT b.*, c.length_limit_m, c.thickness_limit_m
    FROM player_game_batting b CROSS JOIN config c
    WHERE b.game_id BETWEEN c.first_game_id AND c.last_game_id
      AND (b.decision_swung=1 OR b.adjusted_contact_type IN
        ('SwungAndMiss','SolidContact','WeakContact','MarginalContact'))
), classified AS (
    SELECT *,
        CASE WHEN length_offset_m IS NULL OR thickness_offset_m IS NULL
                       OR length_offset_m<0 OR thickness_offset_m<0 THEN 'unknown_geometry'
             WHEN length_offset_m>length_limit_m AND thickness_offset_m>thickness_limit_m THEN 'both_exceeded'
             WHEN length_offset_m>length_limit_m THEN 'length_only'
             WHEN thickness_offset_m>thickness_limit_m THEN 'thickness_only'
             ELSE 'within_limits' END AS geometry_group,
        CASE WHEN decision_count_status IN ('C02','C12','C22','C32') THEN 'two_strikes'
             WHEN decision_count_status IN ('C00','C01','C10','C11','C20','C21','C30','C31') THEN 'before_two_strikes'
             ELSE 'unknown_count' END AS count_group,
        CASE WHEN decision_location_x IS NULL OR decision_location_y IS NULL THEN 'unknown_zone'
             WHEN ABS(decision_location_x)<=1 AND ABS(decision_location_y)<=1 THEN 'in_zone'
             ELSE 'outside_zone' END AS zone_group,
        CASE WHEN adapted_timing_offset_sec IS NULL THEN 'unknown_timing'
             WHEN ABS(adapted_timing_offset_sec)<0.002 THEN '00_<2ms'
             WHEN ABS(adapted_timing_offset_sec)<0.005 THEN '01_2..<5ms'
             WHEN ABS(adapted_timing_offset_sec)<0.010 THEN '02_5..<10ms'
             WHEN ABS(adapted_timing_offset_sec)<0.015 THEN '03_10..<15ms'
             ELSE '04_15ms+' END AS timing_band
    FROM swings
), diagnosed AS (
    SELECT *, CASE
        WHEN geometry_group='unknown_geometry' THEN 'unknown'
        WHEN adjusted_contact_type='SwungAndMiss' AND geometry_group='within_limits' THEN 'miss_within_limits'
        WHEN adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
             AND geometry_group<>'within_limits' THEN 'contact_outside_limits'
        WHEN adjusted_contact_type NOT IN ('SwungAndMiss','SolidContact','WeakContact','MarginalContact') THEN 'unknown_contact_type'
        ELSE 'consistent' END AS consistency
    FROM classified
)
SELECT game_id,count_seq,batter_id,decision_count_status,count_group,zone_group,timing_band,
 geometry_group,consistency,decision_location_x,decision_location_y,
 adapted_timing_offset_sec,timing_impact_x_m,adapted_horizontal_offset_m,adapted_vertical_offset_m,
 swing_error_x_m,swing_error_z_m,contact_offset_x_m,contact_offset_z_m,
 length_offset_m,thickness_offset_m,adjusted_contact_type,result
FROM diagnosed WHERE adjusted_contact_type='SwungAndMiss'
ORDER BY game_id,count_seq;
