SELECT
    *
FROM
    player_game_fielding pgf
WHERE
    EXISTS (
        SELECT
            game_id,
            count_seq
        FROM
            player_game_batting pgb
        WHERE
            result = 'Single'
            AND fielder_position IN ('LF', 'CF', 'RF')
            AND launch_speed >= 25
            AND launch_speed < 35
            AND launch_angle >= 25
            AND first_bounce_time IS NOT NULL
            AND fence_impact_time IS NULL
            AND pgf.game_id = pgb.game_id
            AND pgf.count_seq = pgb.count_seq
    )