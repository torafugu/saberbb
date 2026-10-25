WITH counts AS (
    SELECT
        COUNT(*) AS pitches,
        SUM(
            CASE
                WHEN adjusted_contact_type IN (
                    'SwungAndMiss',
                    'SolidContact',
                    'WeakContact',
                    'MarginalContact'
                ) THEN 1
                ELSE 0
            END
        ) AS swings,
        SUM(
            CASE
                WHEN adjusted_contact_type IN (
                    'SolidContact',
                    'WeakContact',
                    'MarginalContact'
                ) THEN 1
                ELSE 0
            END
        ) AS contacts,
        SUM(
            CASE
                WHEN adjusted_contact_type = 'SwungAndMiss' THEN 1
                ELSE 0
            END
        ) AS whiffs
    FROM
        player_game_batting -- WHERE game_id BETWEEN ... AND ...
)
SELECT
    pitches,
    swings,
    contacts,
    whiffs,
    ROUND(100.0 * swings / NULLIF(pitches, 0), 2) AS swing_pct,
    ROUND(100.0 * contacts / NULLIF(swings, 0), 2) AS contact_pct,
    ROUND(100.0 * whiffs / NULLIF(swings, 0), 2) AS whiff_pct
FROM
    counts;