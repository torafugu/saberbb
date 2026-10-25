WITH counts AS (
    SELECT
        adjusted_contact_type,
        result,
        COUNT(*) AS n
    FROM
        player_game_batting
    GROUP BY
        adjusted_contact_type,
        result
)
SELECT
    adjusted_contact_type,
    result,
    n,
    ROUND(
        100.0 * n / SUM(n) OVER (PARTITION BY adjusted_contact_type),
        2
    ) AS pct_within_type
FROM
    counts
ORDER BY
    adjusted_contact_type,
    n DESC;