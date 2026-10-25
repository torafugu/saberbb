CREATE TABLE IF NOT EXISTS player_game_fielding_candidate (
    game_id INTEGER NOT NULL,
    count_seq INTEGER NOT NULL,
    fielder_position TEXT NOT NULL,
    fielder_id INTEGER NOT NULL,
    lane_eligible INTEGER NOT NULL CHECK (lane_eligible IN (0, 1)),
    distance_eligible INTEGER NOT NULL CHECK (distance_eligible IN (0, 1)),
    evaluated INTEGER NOT NULL CHECK (evaluated IN (0, 1)),
    air_samples INTEGER,
    height_ok_samples INTEGER,
    reachable_samples INTEGER,
    risk_ok_samples INTEGER,
    max_air_margin_sec REAL,
    max_air_margin_time_sec REAL,
    max_air_margin_height_m REAL,
    required_margin_sec REAL NOT NULL,
    last_evaluated_time_sec REAL,
    end_reason TEXT NOT NULL,
    interception_catch_type TEXT,
    interception_error_type TEXT,
    interception_time_sec REAL,
    selected INTEGER NOT NULL CHECK (selected IN (0, 1)),
    selected_path TEXT,
    air_probe_step_sec REAL CHECK (air_probe_step_sec > 0),
    air_probe_air_samples INTEGER CHECK (air_probe_air_samples >= 0),
    air_probe_height_ok_samples INTEGER CHECK (air_probe_height_ok_samples >= 0),
    air_probe_reachable_samples INTEGER CHECK (air_probe_reachable_samples >= 0),
    air_probe_risk_ok_samples INTEGER CHECK (air_probe_risk_ok_samples >= 0),
    air_probe_max_margin_sec REAL,
    air_probe_max_margin_time_sec REAL,
    air_probe_max_margin_height_m REAL,
    air_probe_first_catch_time_sec REAL,
    air_probe_first_catch_height_m REAL,
    air_probe_first_catch_margin_sec REAL,
    air_probe_coarse_step_sec REAL CHECK (air_probe_coarse_step_sec > 0),
    air_probe_coarse_air_samples INTEGER CHECK (air_probe_coarse_air_samples >= 0),
    air_probe_coarse_height_ok_samples INTEGER CHECK (air_probe_coarse_height_ok_samples >= 0),
    air_probe_coarse_reachable_samples INTEGER CHECK (air_probe_coarse_reachable_samples >= 0),
    air_probe_coarse_risk_ok_samples INTEGER CHECK (air_probe_coarse_risk_ok_samples >= 0),
    air_probe_coarse_max_margin_sec REAL,
    air_probe_coarse_max_margin_time_sec REAL,
    air_probe_coarse_max_margin_height_m REAL,
    air_probe_coarse_first_catch_time_sec REAL,
    air_probe_coarse_first_catch_height_m REAL,
    air_probe_coarse_first_catch_margin_sec REAL,
    PRIMARY KEY (game_id, count_seq, fielder_position),
    CHECK (evaluated = (lane_eligible AND distance_eligible)),
    CHECK (
        (evaluated = 0 AND air_samples IS NULL AND height_ok_samples IS NULL
            AND reachable_samples IS NULL AND risk_ok_samples IS NULL)
        OR
        (evaluated = 1 AND air_samples IS NOT NULL AND height_ok_samples IS NOT NULL
            AND reachable_samples IS NOT NULL AND risk_ok_samples IS NOT NULL
            AND air_samples >= height_ok_samples AND height_ok_samples >= reachable_samples
            AND reachable_samples >= risk_ok_samples AND risk_ok_samples >= 0)
    ),
    CHECK ((selected = 0 AND selected_path IS NULL)
        OR (selected = 1 AND selected_path IS NOT NULL AND selected_path IN ('interception', 'covering', 'final_pickup')))
);
