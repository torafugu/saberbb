DROP TABLE test_pitched_offset;

CREATE TABLE test_pitched_offset (
    pitch_type TEXT NOT NULL,
    speed_kmh REAL NOT NULL,
    base_disp_x REAL NOT NULL,
    base_disp_y REAL NOT NULL,
    late_break_x REAL NOT NULL,
    late_break_y REAL NOT NULL,
    enhanced_late_break_x REAL NOT NULL,
    final_x REAL NOT NULL,
    final_y REAL NOT NULL,
    timing REAL NOT NULL,
    norm_x REAL NOT NULL,
    norm_y REAL NOT NULL,
    pitch_result TEXT NOT NULL
);