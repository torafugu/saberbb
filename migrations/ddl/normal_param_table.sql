DROP TABLE normal_param;

CREATE TABLE normal_param (
    category1 TEXT,
    category2 TEXT,
    name TEXT,
    mean REAL NOT NULL,
    std_dev REAL NOT NULL,
    skew REAL NOT NULL,
    coefficient REAL NOT NULL,
    offset REAL NOT NULL,
    PRIMARY KEY (category1, category2, name)
);