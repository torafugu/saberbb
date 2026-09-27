SELECT
    pitching_strategy,
    COUNT(*) AS count,
    ROUND(100.0 * COUNT(*) / SUM(COUNT(*)) OVER (), 2) AS percentage
FROM
    test_select_pitching_strategy
GROUP BY
    pitching_strategy
ORDER BY
    count DESC,
    pitching_strategy;
