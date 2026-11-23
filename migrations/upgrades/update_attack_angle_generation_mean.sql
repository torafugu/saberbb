-- Update the previous stock distribution while preserving customized parameters.
UPDATE normal_param
SET mean = 5.5
WHERE category1 = 'player'
  AND category2 = 'batter_info'
  AND name = 'attack_angle'
  AND mean = 3.5
  AND std_dev = 2.0
  AND skew = 0.3
  AND coefficient = 1.0
  AND offset = 0.0;
