WITH batted AS (
    SELECT
        base_hla_deg,
        final_hla_deg,
        result
    FROM
        player_game_batting
    WHERE
        result IN (
            'Out',
            'Single',
            'Double',
            'Triple',
            'HomeRun',
            'DoublePlay',
            'FieldersChoice',
            'Foul'
        )
)
SELECT
    COUNT(*) AS n,
    SUM(
        CASE
            WHEN (
                ABS(base_hla_deg) > 45.0
            ) = (result = 'Foul') THEN 1
            ELSE 0
        END
    ) AS base_matches_result,
    SUM(
        CASE
            WHEN (
                ABS(final_hla_deg) > 45.0
            ) = (result = 'Foul') THEN 1
            ELSE 0
        END
    ) AS final_matches_result,
    SUM(
        CASE
            WHEN (
                ABS(base_hla_deg) > 45.0
            ) <> (result = 'Foul') THEN 1
            ELSE 0
        END
    ) AS base_mismatches_result,
    SUM(
        CASE
            WHEN (
                ABS(final_hla_deg) > 45.0
            ) <> (result = 'Foul') THEN 1
            ELSE 0
        END
    ) AS final_mismatches_result
FROM
    batted;