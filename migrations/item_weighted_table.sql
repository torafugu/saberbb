DROP TABLE item_weighted;

CREATE TABLE item_weighted (
    category1 TEXT,
    category2 TEXT,
    name TEXT,
    weight REAL NOT NULL,
    PRIMARY KEY (category1, category2, name)
);