SELECT
  COUNT(*) AS n,
  ROUND(AVG(offset_x_m), 4) AS mean_offset_x_m,
  ROUND(
    AVG(offset_x_m * offset_x_m) - AVG(offset_x_m) * AVG(offset_x_m),
    6
  ) AS varp_offset_x_m,
  ROUND(AVG(offset_z_m), 4) AS mean_offset_z_m,
  ROUND(
    AVG(offset_z_m * offset_z_m) - AVG(offset_z_m) * AVG(offset_z_m),
    6
  ) AS varp_offset_z_m,
  ROUND(AVG(actual_bat_angle_deg), 4) AS mean_actual_bat_angle_deg,
  ROUND(
    AVG(actual_bat_angle_deg * actual_bat_angle_deg) - AVG(actual_bat_angle_deg) * AVG(actual_bat_angle_deg),
    6
  ) AS varp_actual_bat_angle_deg
FROM
  test_batted_ball;