DROP TABLE gamma_param;

CREATE TABLE gamma_param (
    category1 TEXT,
    category2 TEXT,
    name TEXT,
    shape REAL NOT NULL,
    scale REAL NOT NULL,
    offset REAL NOT NULL,
    PRIMARY KEY (category1, category2, name)
);