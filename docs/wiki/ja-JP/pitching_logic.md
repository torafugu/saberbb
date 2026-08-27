# 投球ロジック

# 投球処理の流れ
<img src="../images/pitching_diagram.png" width="50%">

# 1. calculate_hanging_pitch_effect

各球に最大10%のランダム変動を適用します。

以下の場合に hanging_pitch（失投）が発生します。
- 投手が調子を崩している場合

# 2. create_pitch
### 2.1 spin angle

#### アームスロット別の基本軸

| TH | TH |
| ---- | ---- |
| TD | TD |
| TD | TD |

### 2.2 pitch skill

### 2.3 pitch calling

### 2.4 calculate_release_point

#### Z軸(高さ): 身長にフォーム係数を乗算

| ArmSlot | Factor |
| ---- | ----: |
| Overhand | 1.05 |
| ThreeQuarter | 0.95 |
| Sidearm | 0.7 |
| Submarine | 0.4 |

#### X軸(横位置): アームスロットによる横への広がり

| ArmSlot | Factor |
| ---- | ----: |
| Overhand | 0.35 |
| ThreeQuarter | 0.55 |
| Sidearm | 0.85 |
| Submarine | 0.6 |

#### Y軸 (打者までの距離): マウンド板 (18.44m) - エクステンション

18.44 - PitcherInfo.extension

# 3. crossfire_perceived_multiplier

#### 打者の慣れによる入射角の補正

$$\text{HAA}_{\text{perceived}} = \text{HAA}_{\text{physical}} \times \text{UnfamiliarityFactor}$$

- 右投手 vs 右打者 / 左投手 vs 左打者（同型）： $\text{UnfamiliarityFactor}=1.0$（標準）
- 右投手 vs 左打者： $\text{UnfamiliarityFactor}=1.0$（左打者は右投手のクロスに慣れているため補正なし）
- 左投手 vs 右打者： $\text{UnfamiliarityFactor}=1.25 〜 1.35$ （希少性による体感角度の強調）

# 4. calculate_location_bias

# 5. calculate_pitch_offset




# 6. calculate_ball_movement
