-- For Angle
WITH stats AS (
    -- 1. Compute the overall min, max, and the bucket width (step) for 10 buckets
    SELECT
        MIN(spray_angle) AS min_dist,
        MAX(spray_angle) AS max_dist,
        (MAX(spray_angle) - MIN(spray_angle)) / 10.0 AS bucket_width
    FROM
        test_batted_ball
),
converted_data AS (
    -- 2. Determine which bucket (0–9) each data point belongs to
    SELECT
        t.spray_angle,
        s.min_dist,
        s.bucket_width,
        CASE
            WHEN FLOOR((t.spray_angle - s.min_dist) / s.bucket_width) >= 10 THEN 9
            ELSE FLOOR((t.spray_angle - s.min_dist) / s.bucket_width)
        END AS bucket_index
    FROM
        test_batted_ball t,
        stats s
) -- 3. Compute start/end values for each bucket, format as a range string, and aggregate
SELECT
    bucket_index + 1 AS bucket_number,
    -- Build the "start - end" range text (rounded to 1 decimal place)
    ROUND(min_dist + (bucket_index * bucket_width), 1) || ' ... ' || ROUND(
        min_dist + ((bucket_index + 1) * bucket_width),
        1
    ) AS spray_angle_range,
    COUNT(*) AS count
FROM
    converted_data
GROUP BY
    bucket_index,
    min_dist,
    bucket_width
ORDER BY
    bucket_index;

-- For Distance
WITH stats AS (
    -- 1. Compute the overall min, max, and the bucket width (step) for 10 buckets
    SELECT
        MIN(distance) AS min_dist,
        MAX(distance) AS max_dist,
        (MAX(distance) - MIN(distance)) / 10.0 AS bucket_width
    FROM
        test_batted_ball
),
converted_data AS (
    -- 2. Determine which bucket (0–9) each data point belongs to
    SELECT
        t.distance,
        s.min_dist,
        s.bucket_width,
        CASE
            WHEN FLOOR((t.distance - s.min_dist) / s.bucket_width) >= 10 THEN 9
            ELSE FLOOR((t.distance - s.min_dist) / s.bucket_width)
        END AS bucket_index
    FROM
        test_batted_ball t,
        stats s
) -- 3. Compute start/end values for each bucket, format as a range string, and aggregate
SELECT
    bucket_index + 1 AS bucket_number,
    -- Build the "start - end" range text (rounded to 1 decimal place)
    ROUND(min_dist + (bucket_index * bucket_width), 1) || ' - ' || ROUND(
        min_dist + ((bucket_index + 1) * bucket_width),
        1
    ) AS distance_range,
    COUNT(*) AS count
FROM
    converted_data
GROUP BY
    bucket_index,
    min_dist,
    bucket_width
ORDER BY
    bucket_index;