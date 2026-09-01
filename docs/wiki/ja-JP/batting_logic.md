# 打撃ロジック

# 打撃処理の流れ
<img src="../images/batting_diagram.png" width="50%">

# 1. calculate_batting_factor

calculate_swing_factor、および、adapt_to_pitchへの修正値の計算

$$\text{total\_batting\_modifier} =　\frac{(1.0 - \text{zone\_aptitude}) + (1.0 - \text{zone\_similarity}) + (1.0 - \text{pitch\_similarity})}{3.0}$$

## 1.1 distance_from_zone_edge

ボールのストライクゾーンの端$(0,0)$からの距離の計算

## 1.2 calculate_zone_similarity_factor

投球のストライクゾーンの類似度計算

- 予測通り真ん中：+0.3
- 予測とは違うが実際のコースが真ん中：+0.1
- 外角／内角、高め／低め、どちらも予測通り：+0.2
- 外角／内角、または、高め／低め、どちらかが予想通り：+0.05
- 実際のコースが予測の対角：-0.2

## 1.3 calculate_pitch_similarity

球種の類似度計算

- スピン回転数の類似度
  - $\text{spin\_rate\_similarity} = 0.2 - (\text{actual\_spin\_rate} - \text{expected\_spin\_rate})$
- スピン角度の類似度
  - $\text{spin\_angle\_similarity} = 0.2 - \frac{\text{actual\_spin\_angle} - \text{expected\_spin\_angle}}{360}$
- 球種の類似度
  - $\text{pitch\_rate\_similarity} = \text{spin\_angle\_similarity} - \text{spin\_angle\_similarity}$
    - 最大値：0.2
    - 最小値：-0.2

## 1.4 zone_modifier

打者のストライクゾーン別の得意/不得意の修正

$$\text{zone\_modifier} = \text{zone\_aptitude} \cdot (1.0 + \text{hot\_zone\_scale})$$

# 2. adapt_to_pitch

- 空間オフセット値の修正
  - $\text{adapted\_}X_m = \text{offset\_}X_m \cdot \text{total\_modifier}$
  - $\text{adapted\_}X_z = \text{offset\_}Z_m \cdot \text{total\_modifier}$

- タイミングオフセット値の修正
  - $\text{adapted\_timing} = \text{offset\_timing} \cdot \text{total\_modifier}$

# 3. calculate_swing_factor

**1.カウント状態の係数**

| ボール | ストライク | 係数 |
| ----: | ----: | ----: |
| 0 | 0 | 0.0 |
| 1 | 0 | -0.1 |
| 2 | 0 | 0.25 |
| 0 | 1 | 0.1 |
| 1 | 1 | 0.0 |
| 2 | 1 | -0.1 |
| 3 | 1 | -0.25 |
| 0 | 2 | 0.2 |
| 1 | 2 | 0.1 |
| 2 | 2 | 0.05 |
| 3 | 2 | 0.15 |

**2.打者のバッティングアプローチの係数（2ストライクの場合のみ加算）**

| バッティングアプローチ | 係数 |
| ---- | ----: |
| Aggressive | 0.2 |
| Patient | -0.1 |
| Take | -0.5 |

**3.球種による係数**

- ストレート：+0.1
- ストレート以外：-0.05

**４.$\text{swing\_factor}$の合算**

$$\text{swing\_factor} = \text{count\_status\_factor} + \text{fastball\_factor}  + \text{distance\_from\_zone\_edge\_factor} \text{zone\_similarity\_factor} + \text{pitch\_similarity\_factor}  + \text{zone\_aptitude\_factor}$$

# 4. select_swing_execution

乱数が$\text{swing\_factor}$未満ならスイング、$\text{swing\_factor}$以上なら見送り。

# 5. calculate_swing_execution_error

- バット傾斜角の決定
- スイング進入角の決定
- 追加空間オフセット値の決定

## 5.1 calculate_bat_angle

$$\text{bat\_angle\_deg} = \text{CENTER\_ANGLE\_DEG} - (\text{ball\_location\_y} * \text{HIGH\_LOW\_RANGE\_DEG})$$

- $\text{CENTER\_ANGLE\_DEG} = 30^\circ$（ストライクゾーンの真ん中における標準のバット傾斜角）
- $\text{ball\_location\_y} = +1.0 	~ -1.0$（ボールの高さ）
- $\text{HIGH\_LOW\_RANGE\_DEG} = 15^\circ$（ボールの高さによるバット傾斜角の変動値）
- 最大値：60°
- 最小値：10°

## 5.2 calculate_dynamic_attack_angle

$$\text{attack\_angle\_deg} = \text{BatterInfo.attack\_angle} + ((\text{bat\_angle\_deg} - \text{BASE\_BAT\_ANGLE\_DEG}) * \text{COUPLING\_FACTOR})$$

- $\text{BASE\_BAT\_ANGLE\_DEG} = 30^\circ$（バット傾斜角の標準値）
- $\text{COUPLING\_FACTOR} = 0.35$（バット傾斜角とスイング進入角の相関係数）

# 6. evaluate_swing_contact

- バットの長さを越えている場合（芯からバットの端までの長さ方向の距離：0.35m）、SwungAndMiss
- バットの太さを越えている場合（芯からバットの端までの太さ方向の距離：0.07m）、SwungAndMiss
- 芯からバットの端までの太さ方向の距離 > 0.055mの場合、FoulTip
- 芯からバットの端までの太さ方向の距離 > 0.025mの場合、WeakContact
- 芯からバットの端までの太さ方向の距離 > 0.025mの場合、SolidContact

# 7. calculate_batted_ball

打球の計算

## 7.1 calculate_collision_spin

1. バットとボールの衝突地点とバットの芯からの距離を計算
2. 衝突地点の角度からスピン角度を計算
3. スピン回転数を計算
$$\text{raw\_spin\_rate} = \text{MAX\_COLLISION\_SPIN\_AT\_REF\_SPEED} * \frac{\text{swing\_speed}}{\text{REF\_SWING\_SPEED}}$$
- $\text{REF\_SWING\_SPEED} = 33.333$（標準スイング速度 m/s）
- $\text{MAX\_COLLISION\_SPIN\_AT\_REF\_SPEED} = 4000$（標準スイング速度におけるスピン回転数の最大値）
1. combine_batted_spinで投球ベクトルと合算

## 7.2 combine_batted_spin

1. 投球の回転数に$\text{retention\_rate}$を掛けて、打球に加える回転数を計算
2. 投球ベクトルとボールとバットの衝突ベクトルを加えて打球ベクトルを計算

## 7.3 calculate_effective_c_swing

打球の加速度に影響を与える打者のパワー値

$$\text{effective\_}C_{\text{swing}} = \begin{cases} C_{\text{swing}} \cdot (1.0 + \frac{X_m}{0.2} \cdot 0.05) & (X_m < 0.0) \\ C_{\text{swing}} \cdot (1.0 - \frac{X_m}{0.2} \cdot 0.15) & (X_m \ge 0.0) \end{cases}$$

## 7.4 calculate_launch_speed_with_power

1. バットの芯を完全に捉えた場合の最大加速度の計算
$$\text{c\_swing} = 1.12 + (0.16 \cdot power)$$
- $power$（calculate_effective_c_swingで打者のパワー値）
$$\text{max\_launch\_speed} = (\text{C\_PITCH} * \text{ball\_speed}) + (\text{c\_swing} * \text{swing\_speed})$$
- $\text{C\_PITCH}$（投球スピードの打球スピードへの寄与率）

2. バットの芯からの太さ方向の距離による減衰率の計算
$$\text{e\_thick} = \begin{cases} 1.0 & (Y_m \le \text{SWEET\_SPOT\_RADIUS}) \\ 0.0 & (Y_m \ge \text{MAX\_CONTACT\_RADIUS}) \\ \frac{Y_m - \text{SWEET\_SPOT\_RADIUS}}{\text{MAX\_CONTACT\_RADIUS} - \text{SWEET\_SPOT\_RADIUS}} & (Y_m > \text{SWEET\_SPOT\_RADIUS} \quad \& \quad Y_m < \text{MAX\_CONTACT\_RADIUS})\end{cases}$$
- $\text{SWEET\_SPOT\_RADIUS} = 0.02$（バットの芯の範囲）
- $\text{MAX\_CONTACT\_RADIUS} = 0.07$（バットの太さ方向の接触範囲）

3. バットの芯からの長さ方向の距離による減衰率の計算

$$\text{e\_len} = 1.0 - (X_m / \text{MAX\_LENGTH\_OFFSET})^2$$
- $\text{MAX\_LENGTH\_OFFSET} = 0.35$（バットの長さ方向の接触範囲）

4. 打球の加速度の計算

$$\text{launch\_speed} = \text{max\_launch\_speed} \cdot \text{e\_thick} \cdot \text{e\_len}$$

## 7.5 calculate_launch_angles

1. 垂直打出し角の計算
$$\text{vla\_deg} = \text{attack\_angle\_deg} + (\text{normal\_angle\_z} * \text{VLA\_REBOUND\_FACTOR})$$
- $\text{attack\_angle\_deg}$（スイング進入角）
- $\text{normal\_angle\_z}$（バットの芯からのズレによる係数）
- $\text{VLA\_REBOUND\_FACTOR}$（衝突時のボールのたわみによる係数）

2. 水平打ち出し角の計算
$$\text{hla\_deg} = (\text{face\_angle\_rad} * \text{HLA\_FACE\_FACTOR}) + (\text{rebound\_angle\_x} * \text{HLA\_REBOUND\_FACTOR})$$

- $\text{face\_angle\_rad}$（バットの回転によって生じる角度）
- $\text{HLA\_FACE\_FACTOR}$（$\text{face\_angle\_rad}$の水平打ち出し角への寄与率）
- $\text{rebound\_angle\_x}$（バットとボールの反発によって生じる角度）
- $\text{HLA\_REBOUND\_FACTOR}$（$\text{rebound\_angle\_x}$の水平打ち出し角への寄与率）


## 7.6 calculate_trajectory