-- For Distance
WITH stats AS (
    -- 1. Compute the overall min, max, and the bucket width (step) for 10 buckets
    SELECT
        MIN(distance_m) AS min_dist,
        MAX(distance_m) AS max_dist,
        (MAX(distance_m) - MIN(distance_m)) / 10.0 AS bucket_width
    FROM
        test_batted_ball
),
converted_data AS (
    -- 2. Determine which bucket (0–9) each data point belongs to
    SELECT
        t.distance_m,
        s.min_dist,
        s.bucket_width,
        CASE
            WHEN FLOOR((t.distance_m - s.min_dist) / s.bucket_width) >= 10 THEN 9
            ELSE FLOOR((t.distance_m - s.min_dist) / s.bucket_width)
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
    ) AS distance_m_range,
    COUNT(*) AS count
FROM
    converted_data
GROUP BY
    bucket_index,
    min_dist,
    bucket_width
ORDER BY
    bucket_index;