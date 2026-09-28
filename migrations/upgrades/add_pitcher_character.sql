-- Run once on an existing database created before pitcher_character was added.
BEGIN TRANSACTION;
ALTER TABLE pitcher_info ADD COLUMN pitcher_character TEXT NOT NULL DEFAULT 'Balanced';
INSERT INTO item_weighted (category1, category2, name, weight) VALUES
    ('pitcher_info', 'pitcher_character', 'Aggressive', 0.25),
    ('pitcher_info', 'pitcher_character', 'Cautious', 0.25),
    ('pitcher_info', 'pitcher_character', 'Flexible', 0.25),
    ('pitcher_info', 'pitcher_character', 'Balanced', 0.25);
COMMIT;
