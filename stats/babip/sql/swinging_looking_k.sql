WITH counts AS (
    SELECT
        SUM(
            CASE
                WHEN result IN (
                    'Strikeout',
                    'Out',
                    'DoublePlay',
                    'FieldersChoice',
                    'Single',
                    'Double',
                    'Triple',
                    'HomeRun',
                    'Walk',
                    'HitByPitch'
                ) THEN 1
                ELSE 0
            END
        ) AS plate_appearances,
        SUM(
            CASE
                WHEN result = 'Strikeout' THEN 1
                ELSE 0
            END
        ) AS strikeouts,
        SUM(
            CASE
                WHEN result = 'Strikeout'
                AND adjusted_contact_type = 'SwungAndMiss' THEN 1
                ELSE 0
            END
        ) AS swinging_strikeouts,
        SUM(
            CASE
                WHEN result = 'Strikeout'
                AND adjusted_contact_type = 'Take' THEN 1
                ELSE 0
            END
        ) AS looking_strikeouts,
        SUM(
            CASE
                WHEN result = 'Strikeout'
                AND (
                    adjusted_contact_type IS NULL
                    OR adjusted_contact_type NOT IN ('SwungAndMiss', 'Take')
                ) THEN 1
                ELSE 0
            END
        ) AS other_strikeouts
    FROM
        player_game_batting -- WHERE game_id BETWEEN ... AND ...
)
SELECT
    plate_appearances,
    strikeouts,
    swinging_strikeouts,
    looking_strikeouts,
    other_strikeouts,
    ROUND(
        100.0 * strikeouts / NULLIF(plate_appearances, 0),
        2
    ) AS k_pct,
    ROUND(
        100.0 * swinging_strikeouts / NULLIF(plate_appearances, 0),
        2
    ) AS swinging_k_pct,
    ROUND(
        100.0 * looking_strikeouts / NULLIF(plate_appearances, 0),
        2
    ) AS looking_k_pct,
    ROUND(
        100.0 * swinging_strikeouts / NULLIF(strikeouts, 0),
        2
    ) AS swinging_share_of_k_pct,
    ROUND(
        100.0 * looking_strikeouts / NULLIF(strikeouts, 0),
        2
    ) AS looking_share_of_k_pct
FROM
    counts;