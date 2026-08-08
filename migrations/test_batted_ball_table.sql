DROP TABLE test_batted_ball;

CREATE TABLE test_batted_ball (
    offset_x_m REAL NOT NULL,
    offset_z_m REAL NOT NULL,
    thickness_offset_m REAL NOT NULL,
    length_offset_m REAL NOT NULL,
    timing_offset_sec REAL NOT NULL,
    contact_type TEXT NOT NULL,
    launch_speed_kmh REAL NOT NULL,
    launch_angle REAL NOT NULL,
    spray_angle REAL NOT NULL,
    distance_m REAL NOT NULL,
    hang_time_sec REAL NOT NULL,
    trajectory TEXT NOT NULL
);