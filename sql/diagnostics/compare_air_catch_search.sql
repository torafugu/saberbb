-- Bind the four named game-ID parameters; keep before/after ranges disjoint.

-- Pitch/result distribution and fair-ball hit rate
WITH ranges(cohort, first_game_id, last_game_id) AS (
    VALUES ('before', :before_first_game_id, :before_last_game_id),
           ('after', :after_first_game_id, :after_last_game_id)
)
SELECT r.cohort, b.result, COUNT(*) AS n,
       ROUND(100.0 * COUNT(*) / SUM(COUNT(*)) OVER (PARTITION BY r.cohort), 3) AS pct_all_pitches
FROM ranges r JOIN player_game_batting b
  ON b.game_id BETWEEN r.first_game_id AND r.last_game_id
GROUP BY r.cohort, b.result
ORDER BY r.cohort, n DESC;

-- Fair balls, hits and BABIP-style rate
WITH ranges(cohort, first_game_id, last_game_id) AS (
    VALUES ('before', :before_first_game_id, :before_last_game_id),
           ('after', :after_first_game_id, :after_last_game_id)
)
SELECT r.cohort,
       SUM(CASE WHEN b.result IN ('Out','Single','Double','Triple','HomeRun','DoublePlay','FieldersChoice')
                THEN 1 ELSE 0 END) AS fair_balls,
       SUM(CASE WHEN b.result IN ('Single','Double','Triple','HomeRun') THEN 1 ELSE 0 END) AS hits,
       SUM(CASE WHEN b.result = 'Single' THEN 1 ELSE 0 END) AS singles,
       ROUND(100.0 * SUM(CASE WHEN b.result IN ('Single','Double','Triple') THEN 1 ELSE 0 END)
           / NULLIF(SUM(CASE WHEN b.result IN ('Out','Single','Double','Triple','DoublePlay','FieldersChoice')
                            THEN 1 ELSE 0 END), 0), 3) AS non_hr_fair_hit_pct
FROM ranges r JOIN player_game_batting b
  ON b.game_id BETWEEN r.first_game_id AND r.last_game_id
GROUP BY r.cohort;

-- Selected outfielder paths: seq=1 avoids duplicate rows for additional throws
WITH ranges(cohort, first_game_id, last_game_id) AS (
    VALUES ('before', :before_first_game_id, :before_last_game_id),
           ('after', :after_first_game_id, :after_last_game_id)
)
SELECT r.cohort, f.interception_fielder_position AS fielder_position,
       f.selection_path, f.catch_type, f.error_type, b.result,
       COUNT(*) AS n, ROUND(AVG(f.waiting_time_sec), 4) AS avg_waiting_time_sec
FROM ranges r JOIN player_game_fielding f
  ON f.game_id BETWEEN r.first_game_id AND r.last_game_id AND f.seq = 1
JOIN player_game_batting b ON b.game_id = f.game_id AND b.count_seq = f.count_seq
WHERE f.interception_fielder_position IN ('LF','CF','RF')
GROUP BY r.cohort, f.interception_fielder_position, f.selection_path, f.catch_type, f.error_type, b.result
ORDER BY r.cohort, fielder_position, n DESC;

-- Outfielder diagnostic gaps, deduplicated by ball; report incomplete probe records separately
WITH ranges(cohort, first_game_id, last_game_id) AS (
    VALUES ('before', :before_first_game_id, :before_last_game_id),
           ('after', :after_first_game_id, :after_last_game_id)
), per_ball AS (
    SELECT r.cohort, c.game_id, c.count_seq,
           COUNT(*) AS outfielder_rows,
           SUM(CASE WHEN c.air_probe_step_sec = 0.01 AND c.air_probe_risk_ok_samples IS NOT NULL
                    THEN 1 ELSE 0 END) AS fine_recorded_rows,
           MAX(CASE WHEN c.air_probe_risk_ok_samples > 0 THEN 1 ELSE 0 END) AS probe_possible,
           MAX(CASE WHEN c.evaluated = 0 AND c.air_probe_risk_ok_samples > 0
                    THEN 1 ELSE 0 END) AS excluded_but_probe_possible,
           MAX(CASE WHEN c.evaluated = 1 AND c.risk_ok_samples = 0
                         AND c.air_probe_risk_ok_samples > 0 THEN 1 ELSE 0 END)
                    AS normal_failed_but_probe_possible
    FROM ranges r JOIN player_game_fielding_candidate c
      ON c.game_id BETWEEN r.first_game_id AND r.last_game_id
    WHERE c.fielder_position IN ('LF','CF','RF')
    GROUP BY r.cohort, c.game_id, c.count_seq
)
SELECT p.cohort, b.result, COUNT(*) AS recorded_balls,
       SUM(CASE WHEN p.outfielder_rows = 3 AND p.fine_recorded_rows = 3 THEN 1 ELSE 0 END) AS complete_probe_balls,
       SUM(CASE WHEN p.outfielder_rows = 3 AND p.fine_recorded_rows = 3 THEN p.probe_possible END) AS probe_possible,
       SUM(CASE WHEN p.outfielder_rows = 3 AND p.fine_recorded_rows = 3 THEN p.excluded_but_probe_possible END) AS excluded_but_probe_possible,
       SUM(CASE WHEN p.outfielder_rows = 3 AND p.fine_recorded_rows = 3 THEN p.normal_failed_but_probe_possible END) AS normal_failed_but_probe_possible
FROM per_ball p JOIN player_game_batting b ON b.game_id = p.game_id AND b.count_seq = p.count_seq
GROUP BY p.cohort, b.result
ORDER BY p.cohort, b.result;

-- Actual direct flies at the first bounce/fence boundary (expected zero after the fix)
WITH ranges(cohort, first_game_id, last_game_id) AS (
    VALUES ('before', :before_first_game_id, :before_last_game_id),
           ('after', :after_first_game_id, :after_last_game_id)
)
SELECT r.cohort, f.game_id, f.count_seq, f.catch_type, f.error_type,
       f.catch_time_sec, b.first_bounce_time, b.fence_impact_time
FROM ranges r JOIN player_game_fielding f
  ON f.game_id BETWEEN r.first_game_id AND r.last_game_id AND f.seq = 1
JOIN player_game_batting b ON b.game_id = f.game_id AND b.count_seq = f.count_seq
WHERE f.catch_type = 'DirectFly'
  AND ((b.first_bounce_time IS NOT NULL AND f.catch_time_sec >= b.first_bounce_time - 1e-9)
    OR (b.fence_impact_time IS NOT NULL AND f.catch_time_sec >= b.fence_impact_time - 1e-9))
ORDER BY r.cohort, f.game_id, f.count_seq;
