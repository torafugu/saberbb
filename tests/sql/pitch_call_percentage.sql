-- Run after: cargo test --test simulate_pitching test_select_pitching_strategy -- --nocapture
-- Distribution of final calls within each strategy.
SELECT
    pitching_strategy, pitch_type, target_zone, margin,
    COUNT(*) AS count,
    ROUND(100.0 * COUNT(*) / SUM(COUNT(*)) OVER (PARTITION BY pitching_strategy), 2) AS percentage,
    ROUND(AVG(score), 4) AS avg_score
FROM test_select_pitching_strategy
GROUP BY pitching_strategy, pitch_type, target_zone, margin
ORDER BY pitching_strategy, count DESC, pitch_type, target_zone, margin;

-- How often a one-sided candidate was adopted after reevaluation.
SELECT decision_reason, COUNT(*) AS count,
    ROUND(100.0 * COUNT(*) / SUM(COUNT(*)) OVER (), 2) AS percentage
FROM test_select_pitching_strategy
GROUP BY decision_reason
ORDER BY count DESC, decision_reason;
