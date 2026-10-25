WITH counts AS (
    SELECT
        adjusted_contact_type,
        COUNT(*) AS n
    FROM
        player_game_batting
    GROUP BY
        adjusted_contact_type
)
SELECT
    adjusted_contact_type,
    n,
    ROUND(100.0 * n / SUM(n) OVER (), 2) AS pct_all_pitches,
    CASE
        WHEN adjusted_contact_type IN (
            'SolidContact',
            'WeakContact',
            'MarginalContact'
        ) THEN ROUND(
            100.0 * n / NULLIF(
                SUM(
                    CASE
                        WHEN adjusted_contact_type IN (
                            'SolidContact',
                            'WeakContact',
                            'MarginalContact'
                        ) THEN n
                        ELSE 0
                    END
                ) OVER (),
                0
            ),
            2
        )
    END AS pct_of_contacts
FROM
    counts
ORDER BY
    n DESC;