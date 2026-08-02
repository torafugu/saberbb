-- For late break offsets
WITH late_break_values AS (
    SELECT
        'late_break_x' AS metric,
        late_break_x AS value
    FROM
        test_pitched_offset
    UNION ALL
    SELECT
        'late_break_y' AS metric,
        late_break_y AS value
    FROM
        test_pitched_offset
),
stats AS (
    SELECT
        metric,
        MIN(value) AS min_value,
        MAX(value) AS max_value,
        (MAX(value) - MIN(value)) / 10.0 AS bucket_width
    FROM
        late_break_values
    GROUP BY
        metric
),
converted_data AS (
    SELECT
        v.metric,
        v.value,
        s.min_value,
        s.bucket_width,
        CASE
            WHEN s.bucket_width = 0 THEN 0
            WHEN FLOOR((v.value - s.min_value) / s.bucket_width) >= 10 THEN 9
            ELSE CAST(FLOOR((v.value - s.min_value) / s.bucket_width) AS INTEGER)
        END AS bucket_index
    FROM
        late_break_values v
        JOIN stats s ON s.metric = v.metric
)
SELECT
    metric,
    bucket_index + 1 AS bucket_number,
    ROUND(min_value + (bucket_index * bucket_width), 3) || ' ... ' || ROUND(
        min_value + ((bucket_index + 1) * bucket_width),
        3
    ) AS late_break_range,
    COUNT(*) AS count
FROM
    converted_data
GROUP BY
    metric,
    bucket_index,
    min_value,
    bucket_width
ORDER BY
    metric,
    bucket_index;
