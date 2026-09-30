-- Run once on an existing database without catcher calling-style weights.
BEGIN TRANSACTION;
-- Provisional equal weights for catcher calling styles.
INSERT INTO item_weighted (category1, category2, name, weight) VALUES
    ('catcher_info', 'calling_style', 'Balanced', 0.25),
    ('catcher_info', 'calling_style', 'Aggressive', 0.25),
    ('catcher_info', 'calling_style', 'Cautious', 0.25),
    ('catcher_info', 'calling_style', 'Adaptive', 0.25);
COMMIT;
