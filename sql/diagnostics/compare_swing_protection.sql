-- Bind disjoint before/after game ranges. Run complete games, not partial at-bats.
WITH ranges(cohort, first_id, last_id) AS (
 VALUES ('before', :before_first_game_id, :before_last_game_id),
        ('after', :after_first_game_id, :after_last_game_id)
), pitches AS (
 SELECT r.cohort, b.*,
   CASE WHEN ABS(decision_location_x)<=1 AND ABS(decision_location_y)<=1 THEN 1 ELSE 0 END AS in_zone,
   CASE WHEN decision_count_status IN ('C02','C12','C22','C32') THEN 1 ELSE 0 END AS two_strikes,
   CASE WHEN result IN ('Strikeout','Walk','HitByPitch','Single','Double','Triple','HomeRun','Out','DoublePlay','FieldersChoice')
        THEN 1 ELSE 0 END AS terminal
 FROM ranges r JOIN player_game_batting b ON b.game_id BETWEEN r.first_id AND r.last_id
)
SELECT cohort, COUNT(*) AS pitches,
 SUM(CASE WHEN decision_count_status IS NOT NULL THEN 1 ELSE 0 END) AS decision_recorded_pitches,
 SUM(terminal) AS plate_appearances,
 SUM(CASE WHEN result='Strikeout' THEN 1 ELSE 0 END) AS strikeouts,
 ROUND(100.0*SUM(CASE WHEN result='Strikeout' THEN 1 ELSE 0 END)/NULLIF(SUM(terminal),0),2) AS k_pct,
 SUM(CASE WHEN result='Strikeout' AND decision_swung=0 THEN 1 ELSE 0 END) AS looking_strikeouts,
 SUM(CASE WHEN result='Strikeout' AND decision_swung=1 THEN 1 ELSE 0 END) AS swinging_strikeouts,
 ROUND(100.0*AVG(CASE WHEN two_strikes=1 AND in_zone=1 THEN decision_swung END),2) AS two_strike_zone_swing_pct,
 ROUND(100.0*SUM(CASE WHEN two_strikes=1 AND in_zone=1 AND decision_swung=1 AND adjusted_contact_type IN
    ('SolidContact','WeakContact','MarginalContact') THEN 1 ELSE 0 END)
    /NULLIF(SUM(CASE WHEN two_strikes=1 AND in_zone=1 THEN decision_swung END),0),2) AS two_strike_zone_contact_pct,
 ROUND(100.0*AVG(CASE WHEN two_strikes=1 AND in_zone=0 THEN decision_swung END),2) AS two_strike_outside_swing_pct
FROM pitches GROUP BY cohort ORDER BY cohort;
