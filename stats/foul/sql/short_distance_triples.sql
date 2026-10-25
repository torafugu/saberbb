SELECT
    ROUND(polar_distance, 2),
    ROUND(first_bounce_distance, 2),
    ROUND(time_to_fields, 2),
    ROUND(defense_time, 2),
    throw_target_base
FROM
    player_game_batting pgb,
    player_game_fielding pgf,
    player_game_running pgr
WHERE
    pgb.result = 'Triple'
    AND pgb.polar_distance < 50.0
    AND pgb.game_id = pgf.game_id
    AND pgb.count_seq = pgf.count_seq
    AND pgf.game_id = pgr.game_id
    AND pgf.count_seq = pgr.count_seq
    AND pgf.seq = pgr.seq