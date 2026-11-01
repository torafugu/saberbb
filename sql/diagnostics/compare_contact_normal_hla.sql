-- Bind disjoint before/after complete-game ranges.

-- 1. Contact quality and actual outcomes
WITH ranges(cohort,lo,hi) AS (
 VALUES ('before',:before_first_game_id,:before_last_game_id),('after',:after_first_game_id,:after_last_game_id)
), contacts AS (
 SELECT r.cohort,b.* FROM ranges r JOIN player_game_batting b ON b.game_id BETWEEN lo AND hi
 WHERE adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
)
SELECT cohort,COALESCE(hla_model_version,1) AS hla_version,adjusted_contact_type,COUNT(*) AS contacts,
 SUM(CASE WHEN result='Foul' THEN 1 ELSE 0 END) AS result_fouls,
 ROUND(100.0*AVG(CASE WHEN result='Foul' THEN 1 ELSE 0 END),2) AS foul_pct,
 ROUND(100.0*AVG(CASE WHEN ABS(final_hla_deg)>=45 THEN 1 ELSE 0 END),2) AS launch_foul_direction_pct,
 ROUND(AVG(launch_speed),3) AS avg_ev_ms,ROUND(AVG(launch_angle),3) AS avg_vla_deg,
 SUM(CASE WHEN result IN ('Single','Double','Triple','HomeRun') THEN 1 ELSE 0 END) AS hits,
 SUM(CASE WHEN result='HomeRun' THEN 1 ELSE 0 END) AS home_runs
FROM contacts GROUP BY cohort,hla_version,adjusted_contact_type;

-- 2. HLA by actual batting orientation
WITH ranges(cohort,lo,hi) AS (
 VALUES ('before',:before_first_game_id,:before_last_game_id),('after',:after_first_game_id,:after_last_game_id)
), contacts AS (
 SELECT r.cohort,b.* FROM ranges r JOIN player_game_batting b ON b.game_id BETWEEN lo AND hi
 WHERE adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
)
SELECT cohort,COALESCE(hla_model_version,1) AS hla_version,adjusted_contact_type,hla_handedness_sign,
 CASE WHEN ABS(final_hla_deg)<15 THEN 'abs<15' WHEN ABS(final_hla_deg)<30 THEN '15..<30'
      WHEN ABS(final_hla_deg)<45 THEN '30..<45' WHEN ABS(final_hla_deg)<90 THEN '45..<90' ELSE '90+' END AS abs_hla_band,
 COUNT(*) AS contacts,ROUND(100.0*COUNT(*)/SUM(COUNT(*)) OVER(PARTITION BY cohort,COALESCE(hla_model_version,1),adjusted_contact_type,hla_handedness_sign),2) AS pct
FROM contacts GROUP BY cohort,hla_version,adjusted_contact_type,hla_handedness_sign,abs_hla_band;

-- 3. Joint EV/VLA distribution including fouls
WITH ranges(cohort,lo,hi) AS (
 VALUES ('before',:before_first_game_id,:before_last_game_id),('after',:after_first_game_id,:after_last_game_id)
), contacts AS (
 SELECT r.cohort,b.* FROM ranges r JOIN player_game_batting b ON b.game_id BETWEEN lo AND hi
 WHERE adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
)
SELECT cohort,COALESCE(hla_model_version,1) AS hla_version,adjusted_contact_type,
 CASE WHEN launch_speed<25 THEN '<25' WHEN launch_speed<35 THEN '25..<35'
      WHEN launch_speed<40 THEN '35..<40' WHEN launch_speed<45 THEN '40..<45' ELSE '45+' END AS ev_band,
 CASE WHEN launch_angle<0 THEN '<0' WHEN launch_angle<10 THEN '0..<10' WHEN launch_angle<25 THEN '10..<25'
      WHEN launch_angle<40 THEN '25..<40' ELSE '40+' END AS vla_band,
 COUNT(*) AS contacts,SUM(CASE WHEN result='Foul' THEN 1 ELSE 0 END) AS result_fouls
FROM contacts GROUP BY cohort,hla_version,adjusted_contact_type,ev_band,vla_band;
