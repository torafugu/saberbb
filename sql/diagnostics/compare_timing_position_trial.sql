-- Bind disjoint before/after complete-game ranges.

-- 1. Outcomes and contact quality; NULL legacy scale means 1 only for swings
WITH ranges(cohort, first_id, last_id) AS (
 VALUES ('before',:before_first_game_id,:before_last_game_id),
        ('after',:after_first_game_id,:after_last_game_id)
), pitches AS (
 SELECT r.cohort,b.*,CASE WHEN adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact') THEN 1 ELSE 0 END AS contact,
 CASE WHEN result IN ('Strikeout','Walk','HitByPitch','Single','Double','Triple','HomeRun','Out','DoublePlay','FieldersChoice') THEN 1 ELSE 0 END AS terminal
 FROM ranges r JOIN player_game_batting b ON b.game_id BETWEEN first_id AND last_id
)
SELECT cohort,COUNT(*) AS pitches,SUM(terminal) AS plate_appearances,
 SUM(CASE WHEN result='Strikeout' THEN 1 ELSE 0 END) AS strikeouts,
 ROUND(100.0*SUM(CASE WHEN result='Strikeout' THEN 1 ELSE 0 END)/NULLIF(SUM(terminal),0),2) AS k_pct,
 SUM(CASE WHEN adjusted_contact_type IN ('SwungAndMiss','SolidContact','WeakContact','MarginalContact') THEN 1 ELSE 0 END) AS swings,
 ROUND(100.0*SUM(contact)/NULLIF(SUM(CASE WHEN adjusted_contact_type IN ('SwungAndMiss','SolidContact','WeakContact','MarginalContact') THEN 1 ELSE 0 END),0),2) AS contact_pct,
 SUM(contact) AS contacts,
 ROUND(100.0*SUM(CASE WHEN result='Foul' THEN 1 ELSE 0 END)/NULLIF(SUM(contact),0),2) AS foul_pct_of_contacts,
 ROUND(AVG(CASE WHEN contact=1 THEN launch_speed END),3) AS avg_ev_ms,
 ROUND(AVG(CASE WHEN contact=1 THEN launch_angle END),3) AS avg_vla_deg,
 GROUP_CONCAT(DISTINCT CASE WHEN adjusted_contact_type IN ('SwungAndMiss','SolidContact','WeakContact','MarginalContact') THEN COALESCE(timing_position_scale,1.0) END) AS applied_scales
FROM pitches GROUP BY cohort;

-- 2. ContactType composition and outcomes (includes fouls)
WITH ranges(cohort, first_id, last_id) AS (
 VALUES ('before',:before_first_game_id,:before_last_game_id),
        ('after',:after_first_game_id,:after_last_game_id)
), pitches AS (
 SELECT r.cohort,b.*,CASE WHEN adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact') THEN 1 ELSE 0 END AS contact,
 CASE WHEN result IN ('Strikeout','Walk','HitByPitch','Single','Double','Triple','HomeRun','Out','DoublePlay','FieldersChoice') THEN 1 ELSE 0 END AS terminal
 FROM ranges r JOIN player_game_batting b ON b.game_id BETWEEN first_id AND last_id
)
SELECT cohort,adjusted_contact_type,result,COUNT(*) AS n,
 ROUND(100.0*COUNT(*)/SUM(COUNT(*)) OVER(PARTITION BY cohort,adjusted_contact_type),2) AS pct_within_type
FROM pitches WHERE contact=1
GROUP BY cohort,adjusted_contact_type,result ORDER BY cohort,adjusted_contact_type,n DESC;

-- 3. Joint EV/VLA distribution by ContactType; all contacts including fouls
WITH ranges(cohort, first_id, last_id) AS (
 VALUES ('before',:before_first_game_id,:before_last_game_id),
        ('after',:after_first_game_id,:after_last_game_id)
), pitches AS (
 SELECT r.cohort,b.*,CASE WHEN adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact') THEN 1 ELSE 0 END AS contact,
 CASE WHEN result IN ('Strikeout','Walk','HitByPitch','Single','Double','Triple','HomeRun','Out','DoublePlay','FieldersChoice') THEN 1 ELSE 0 END AS terminal
 FROM ranges r JOIN player_game_batting b ON b.game_id BETWEEN first_id AND last_id
)
SELECT cohort,adjusted_contact_type,
 CASE WHEN launch_speed<25 THEN '<25' WHEN launch_speed<35 THEN '25..<35'
      WHEN launch_speed<40 THEN '35..<40' WHEN launch_speed<45 THEN '40..<45' ELSE '45+' END AS ev_band,
 CASE WHEN launch_angle< -10 THEN '<-10' WHEN launch_angle<0 THEN '-10..<0'
      WHEN launch_angle<10 THEN '0..<10' WHEN launch_angle<25 THEN '10..<25'
      WHEN launch_angle<40 THEN '25..<40' ELSE '40+' END AS vla_band,
 COUNT(*) AS contacts,
 ROUND(100.0*COUNT(*)/SUM(COUNT(*)) OVER(PARTITION BY cohort,adjusted_contact_type),2) AS pct_within_type,
 SUM(CASE WHEN result='Foul' THEN 1 ELSE 0 END) AS fouls,
 SUM(CASE WHEN result IN ('Single','Double','Triple','HomeRun') THEN 1 ELSE 0 END) AS hits
FROM pitches WHERE contact=1
GROUP BY cohort,adjusted_contact_type,ev_band,vla_band ORDER BY cohort,adjusted_contact_type,ev_band,vla_band;
