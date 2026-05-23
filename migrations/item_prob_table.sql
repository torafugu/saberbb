DROP TABLE item_prob;

CREATE TABLE item_prob (
    category TEXT,
    name TEXT,
    prob REAL NOT NULL,
    PRIMARY KEY (group, name)
);