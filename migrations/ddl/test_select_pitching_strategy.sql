DROP TABLE IF EXISTS test_select_pitching_strategy;

CREATE TABLE test_select_pitching_strategy (
    sample INTEGER PRIMARY KEY,
    balls INTEGER NOT NULL CHECK (balls BETWEEN 0 AND 3),
    strikes INTEGER NOT NULL CHECK (strikes BETWEEN 0 AND 2),
    outs INTEGER NOT NULL CHECK (outs BETWEEN 0 AND 2),
    runner_mask INTEGER NOT NULL CHECK (runner_mask BETWEEN 0 AND 7),
    pitching_strategy TEXT NOT NULL,
    pitch_type TEXT NOT NULL,
    target_zone TEXT NOT NULL,
    margin TEXT NOT NULL,
    aim_x REAL NOT NULL,
    aim_y REAL NOT NULL,
    score REAL NOT NULL,
    pitcher_score REAL NOT NULL,
    catcher_score REAL NOT NULL,
    decision_reason TEXT NOT NULL,
    pitcher_candidates INTEGER NOT NULL,
    catcher_candidates INTEGER NOT NULL,
    previous_pitch_type TEXT,
    previous_target_zone TEXT,
    previous_margin TEXT
);
