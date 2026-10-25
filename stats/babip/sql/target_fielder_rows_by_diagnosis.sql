WITH targets AS (
    SELECT
        c.*
    FROM
        player_game_fielding_candidate c
        JOIN player_game_batting b ON b.game_id = c.game_id
        AND b.count_seq = c.count_seq
    WHERE
        b.result = 'Single'
        AND c.fielder_position IN ('LF', 'CF', 'RF')
        AND c.evaluated = 1
        AND c.risk_ok_samples = 0
        AND c.air_probe_risk_ok_samples > 0
),
per_ball AS (
    SELECT
        game_id,
        count_seq,
        COUNT(*) AS target_fielders,
        MAX(
            CASE
                WHEN air_probe_coarse_risk_ok_samples > 0 THEN 1
                ELSE 0
            END
        ) AS coarse_possible
    FROM
        targets
    GROUP BY
        game_id,
        count_seq
    HAVING
        MIN(
            CASE
                WHEN air_probe_step_sec = 0.01
                AND air_probe_coarse_step_sec = 0.05
                AND air_probe_coarse_risk_ok_samples IS NOT NULL THEN 1
                ELSE 0
            END
        ) = 1
),
classified AS (
    SELECT
        *,
        CASE
            WHEN coarse_possible = 1 THEN 'coarse_also_possible'
            ELSE 'sampling_gap_only'
        END AS diagnosis
    FROM
        per_ball
)
SELECT
    diagnosis,
    COUNT(*) AS balls,
    SUM(target_fielders) AS target_fielder_rows
FROM
    classified
GROUP BY
    diagnosis
ORDER BY
    diagnosis;