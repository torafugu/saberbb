# 打撃ロジック

# 打撃処理の流れ
<img src="../images/batting_diagram.png" width="50%">

# 1. calculate_batting_factor

## 1.1 distance_from_zone_edge

## 1.2 calculate_zone_similarity_factor

- 予測通り真ん中：+0.3
- 予測とは違うが実際のコースが真ん中：+0.1
- 外角／内角、高め／低め、どちらも予測通り：+0.2
- 外角／内角、または、高め／低め、どちらかが予想通り：+0.05
- 実際のコースが予測の対角：-0.2


## 1.3 calculate_pitch_similarity

## 1.4 zone_modifier



# 2. adapt_to_pitch

# 3. calculate_swing_factor


# 4. select_swing_execution

## 4.1 calculate_bat_angle

## 4.2 calculate_dynamic_attack_angle

# 5. evaluate_swing_contact

- バットの長さを越えている場合（芯からバットの端までの長さ方向の距離：0.35m）、SwungAndMiss
- バットの太さを越えている場合（芯からバットの端までの太さ方向の距離：0.07m）、SwungAndMiss
- 芯からバットの端までの太さ方向の距離 > 0.055mの場合、FoulTip
- 芯からバットの端までの太さ方向の距離 > 0.025mの場合、WeakContact
- 芯からバットの端までの太さ方向の距離 > 0.025mの場合、SolidContact

# 6. calculate_batted_ball

## 6.1 classify_trajectory_type

## 6.2 calculate_collision_spin

## 6.3 combine_batted_spin

## 6.4 calculate_effective_c_swing

$$C_{\text{swing}} = \begin{cases} C_{\text{swing}} \cdot (1.0 + \frac{X_m}{0.2} \cdot 0.05) & (X_m < 0.0) \\ C_{\text{swing}} \cdot (1.0 - \frac{X_m}{0.2} \cdot 0.15) & (X_m \ge 0.0) \end{cases}$$

## 6.5 calculate_launch_speed_with_power

- $C_{\text{SWING}} = 1.12 + (0.16 \cdot BatterInfo.swing\_power)$

## 6.6 calculate_launch_angles

## 6.7 calculate_3d_flight_path