DELETE FROM
    item_weighted;

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('pitcher_info', 'throw_side', 'Right', 0.9);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('pitcher_info', 'throw_side', 'Left', 0.1);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('pitcher_info', 'arm_slot', 'Overhand', 0.55);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('pitcher_info', 'arm_slot', 'ThreeQuarter', 0.25);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('pitcher_info', 'arm_slot', 'Sidearm', 0.15);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('pitcher_info', 'arm_slot', 'Submarine', 0.05);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('player', 'batting_side', 'Right', 0.7);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('player', 'batting_side', 'Left', 0.3);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('player', 'hitter_tendency', 'Normal', 0.7);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('player', 'hitter_tendency', 'Pull', 0.2);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('player', 'hitter_tendency', 'Spray', 0.1);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Normal', 'Pull', 0.35);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Normal', 'Center', 0.35);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Normal', 'Opposite', 0.15);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Normal', 'FoulPull', 0.08);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'hitter_tendency',
        'Normal',
        'FoulOpposite',
        0.07
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Pull', 'Pull', 0.55);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Pull', 'Center', 0.25);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Pull', 'Opposite', 0.05);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Pull', 'FoulPull', 0.12);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Pull', 'FoulOpposite', 0.03);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Spray', 'Pull', 0.25);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Spray', 'Center', 0.45);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Spray', 'Opposite', 0.25);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Spray', 'FoulPull', 0.02);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('hitter_tendency', 'Spray', 'FoulOpposite', 0.03);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('player', 'fielder_type', 'Outfielder', 0.24);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'player',
        'fielder_type',
        'MiddleInfielder',
        0.12
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'player',
        'fielder_type',
        'CornerInfielder',
        0.12
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('player', 'fielder_type', 'Pitcher', 0.42);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    ('player', 'fielder_type', 'Catcher', 0.1);

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_info',
        'pitcher_style',
        'PowerPitcher',
        0.4
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_info',
        'pitcher_style',
        'FinessePitcher',
        0.1
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_info',
        'pitcher_style',
        'BalancedPitcher',
        0.5
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'PowerPitcher',
        'FourSeamFastball',
        1.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'PowerPitcher',
        'Cutter',
        0.2
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'PowerPitcher',
        'Curveball',
        0.6
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'PowerPitcher',
        'Slider',
        0.5
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'PowerPitcher',
        'Changeup',
        0.3
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'PowerPitcher',
        'Forkball',
        0.2
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'FinessePitcher',
        'FourSeamFastball',
        1.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'FinessePitcher',
        'Cutter',
        0.4
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'FinessePitcher',
        'Curveball',
        1.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'FinessePitcher',
        'Slider',
        0.9
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'FinessePitcher',
        'Changeup',
        0.8
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'FinessePitcher',
        'Forkball',
        0.6
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'BalancedPitcher',
        'FourSeamFastball',
        1.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'BalancedPitcher',
        'Cutter',
        0.3
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'BalancedPitcher',
        'Curveball',
        0.65
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'BalancedPitcher',
        'Slider',
        0.7
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'BalancedPitcher',
        'Changeup',
        0.5
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'pitcher_style',
        'BalancedPitcher',
        'Forkball',
        0.4
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'Outfielder',
        'MiddleInfielder',
        0.05
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'Outfielder',
        'CornerInfielder',
        0.5
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'Outfielder',
        'Pitcher',
        0.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'Outfielder',
        'Catcher',
        0.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'MiddleInfielder',
        'Outfielder',
        0.5
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'MiddleInfielder',
        'CornerInfielder',
        0.9
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'MiddleInfielder',
        'Pitcher',
        0.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'MiddleInfielder',
        'Catcher',
        0.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'CornerInfielder',
        'Outfielder',
        0.5
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'CornerInfielder',
        'MiddleInfielder',
        0.5
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'CornerInfielder',
        'Pitcher',
        0.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'CornerInfielder',
        'Catcher',
        0.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'Pitcher',
        'Outfielder',
        0.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'Pitcher',
        'MiddleInfielder',
        0.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'Pitcher',
        'CornerInfielder',
        0.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'Pitcher',
        'Catcher',
        0.0
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'Catcher',
        'Outfielder',
        0.2
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'Catcher',
        'MiddleInfielder',
        0.01
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'Catcher',
        'CornerInfielder',
        0.3
    );

INSERT INTO
    item_weighted (category1, category2, name, weight)
VALUES
    (
        'multiple_fielder_type',
        'Catcher',
        'Pitcher',
        0.0
    );