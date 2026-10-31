-- Legacy NULL decisions are excluded. Restrict game_id to compare code versions.
WITH pitches AS (
 SELECT *, CASE WHEN MAX(ABS(decision_location_x), ABS(decision_location_y)) <= 0.8 THEN 'zone_core'
                WHEN MAX(ABS(decision_location_x), ABS(decision_location_y)) <= 1.0 THEN 'zone_edge'
                WHEN SQRT(POW(MAX(ABS(decision_location_x)-1,0),2)+POW(MAX(ABS(decision_location_y)-1,0),2)) <= 0.25 THEN 'outside_near'
                ELSE 'outside_far' END AS zone_band
 FROM player_game_batting
 WHERE decision_count_status IS NOT NULL
)
SELECT decision_count_status, zone_band, COUNT(*) AS pitches,
       SUM(decision_swung) AS swings,
       ROUND(100.0*AVG(decision_swung),2) AS swing_pct,
       ROUND(100.0*SUM(CASE WHEN decision_swung=1 AND adjusted_contact_type IN
           ('SolidContact','WeakContact','MarginalContact') THEN 1 ELSE 0 END)
           / NULLIF(SUM(decision_swung),0),2) AS contact_pct,
       ROUND(AVG(decision_probability)*100,2) AS predicted_swing_pct,
       SUM(CASE WHEN result='Strikeout' AND decision_swung=0 THEN 1 ELSE 0 END) AS looking_strikeouts,
       SUM(CASE WHEN result='Strikeout' AND decision_swung=1 THEN 1 ELSE 0 END) AS swinging_strikeouts,
       ROUND(AVG(decision_zone_factor),4) AS avg_zone_factor,
       ROUND(AVG(decision_protection_factor),4) AS avg_protection_factor,
       ROUND(AVG(decision_anticipation_factor),4) AS avg_anticipation_factor
FROM pitches GROUP BY decision_count_status, zone_band
ORDER BY decision_count_status, zone_band;
