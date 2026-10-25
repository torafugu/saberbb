WITH by_ball AS (
    SELECT
        c.game_id,
        c.count_seq,
        MAX(
            CASE
                WHEN c.air_probe_risk_ok_samples > 0 THEN 1
                ELSE 0
            END
        ) AS probe_possible,
        MAX(
            CASE
                WHEN c.evaluated = 0
                AND c.air_probe_risk_ok_samples > 0 THEN 1
                ELSE 0
            END
        ) AS excluded_but_probe_possible,
        MAX(
            CASE
                WHEN c.evaluated = 1
                AND c.risk_ok_samples = 0
                AND c.air_probe_risk_ok_samples > 0 THEN 1
                ELSE 0
            END
        ) AS normal_failed_but_probe_possible
    FROM
        player_game_fielding_candidate AS c
    WHERE
        c.fielder_position IN ('LF', 'CF', 'RF')
        AND c.air_probe_step_sec IS NOT NULL
    GROUP BY
        c.game_id,
        c.count_seq
)
SELECT
    b.result,
    COUNT(*) AS balls,
    SUM(p.probe_possible) AS probe_possible,
    SUM(p.excluded_but_probe_possible) AS excluded_but_probe_possible,
    SUM(p.normal_failed_but_probe_possible) AS normal_failed_but_probe_possible
FROM
    by_ball AS p
    JOIN player_game_batting AS b ON b.game_id = p.game_id
    AND b.count_seq = p.count_seq
GROUP BY
    b.result
ORDER BY
    balls DESC;