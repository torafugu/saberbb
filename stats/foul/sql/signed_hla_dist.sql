WITH contact AS (
    SELECT
        *
    FROM
        player_game_batting
    WHERE
        final_hla_deg IS NOT NULL
),
banded AS (
    SELECT
        CASE
            WHEN final_hla_deg < -90.0 THEN '<-90'
            WHEN final_hla_deg < -60.0 THEN '-90--60'
            WHEN final_hla_deg < -45.0 THEN '-60--45'
            WHEN final_hla_deg < -30.0 THEN '-45--30'
            WHEN final_hla_deg < -15.0 THEN '-30--15'
            WHEN final_hla_deg < 0.0 THEN '-15-0'
            WHEN final_hla_deg < 15.0 THEN '0-15'
            WHEN final_hla_deg < 30.0 THEN '15-30'
            WHEN final_hla_deg < 45.0 THEN '30-45'
            WHEN final_hla_deg < 60.0 THEN '45-60'
            WHEN final_hla_deg < 90.0 THEN '60-90'
            ELSE '90+'
        END AS hla_band,
        CASE
            WHEN final_hla_deg < -90.0 THEN 1
            WHEN final_hla_deg < -60.0 THEN 2
            WHEN final_hla_deg < -45.0 THEN 3
            WHEN final_hla_deg < -30.0 THEN 4
            WHEN final_hla_deg < -15.0 THEN 5
            WHEN final_hla_deg < 0.0 THEN 6
            WHEN final_hla_deg < 15.0 THEN 7
            WHEN final_hla_deg < 30.0 THEN 8
            WHEN final_hla_deg < 45.0 THEN 9
            WHEN final_hla_deg < 60.0 THEN 10
            WHEN final_hla_deg < 90.0 THEN 11
            ELSE 12
        END AS sort_key
    FROM
        contact
)
SELECT
    hla_band,
    COUNT(*) AS n,
    ROUND(
        100.0 * COUNT(*) / SUM(COUNT(*)) OVER (),
        2
    ) AS pct
FROM
    banded
GROUP BY
    hla_band,
    sort_key
ORDER BY
    sort_key;