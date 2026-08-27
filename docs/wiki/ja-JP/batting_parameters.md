# 打撃パラメータ

打撃関連のパラメータは `src/domain/shared/player.rs` で定義され、`src/domain/player_factory.rs` で生成され、`src/domain/player_service.rs` を経由して読み込まれ、`src/repositories/player_repository.rs` によって永続化され、`src/domain/resolver/batting_resolver.rs` で使用されます。

打撃データは主ポジションが投手でないプレイヤーに対してのみ生成・永続化されます。主ポジションが投手のプレイヤーは現在 `offense_skills.batter = None` となります。

## BatterInfo

`BatterInfo` は打席側、アプローチ、ゾーンプロファイル、スイング、コンタクト、安定度の属性を保持します。

| パラメータ | 型 | DB保存先 | 説明 |
| --- | --- | --- | --- |
| `batting_side` | `RL` | `batter_info.batting_side` | 打席側。値は `Right`（右打ち）、`Left`（左打ち）。 |
| `batter_type` | `BatterType` | `batter_info.batter_type` | `sample_plate_approach()` が使用する打席アプローチのタイプ。 |
| `zone_aptitude` | `ZoneAptitude` | `batter_info.zone_aptitude` | `zone_modifier()` が使用する得意・苦手ゾーンのプロファイル。 |
| `hot_zone_scale` | `f64` | `batter_info.hot_zone_scale` | ゾーン適性モディファイアの強さの倍率。 |
| `batting_eye` | `f64` | `batter_info.batting_eye` | 投球タイミングの知覚に使用される球種判別スキル。 |
| `swing_speed` | `f64` | `batter_info.swing_speed` | タイミングへの影響、初速、衝突スピンの計算に使用されるバット速度。 |
| `swing_power` | `f64` | `batter_info.swing_power` | 初速へのスイング寄与をスケーリングするパワー値。 |
| `attack_angle` | `f64` | `batter_info.attack_angle` | 基準となるアタックアングル（度）。 |
| `bat_control` | `f64` | `batter_info.bat_control` | 投球オフセットへの適応とスイング角度誤差の低減に使用されるコンタクト制御スキル。 |
| `consistency` | `f64` | `batter_info.consistency` | 打者レベルの安定度。生成・永続化されますが、現在は打撃解決処理では使用されていません。 |

## 打者タイプ

`BatterType` は打席アプローチとアタックアングルの調整を制御します。

| 値 | 意味 | アタックアングル補正 |
| --- | --- | --- |
| `AggressiveFreeSwinger` | 積極的で感覚的な打者。 | `10.0` |
| `ClassicAnalyst` | 慎重で理論派の打者。 | `2.0` |
| `GameManager` | 状況に応じて対応する打者。 | `3.0` |
| `ClutchHunter` | ハイリスクで長打志向の打者。 | `0.0` |

`default_plate_approach(batter_type)` は現在、すべての打者タイプに対して異なる重みを持つ `PlateApproach::Aggressive` のみを返します。

## ゾーン適性

`ZoneAptitude` は `BatterInfo::zone_modifier(location)` が使用するガウスピークを制御します。

| 値 | 挙動 |
| --- | --- |
| `Balanced` | ゾーン中央に正のピークが1つ。 |
| `InsideDominant` | 中央の正のピークに加えて内角ゾーンのピーク。 |
| `OutsideDominant` | 中央の正のピークに加えて外角ゾーンのピーク。 |
| `LowBaller` | 低めゾーンの正のピークと高めゾーンの負のペナルティ。 |
| `HighBaller` | 高めゾーンの正のピークと低めゾーンの負のペナルティ。 |
| `DiagonalCross` | 対角線上の正のピークと、反対側の対角線の負のペナルティ。 |

`zone_modifier(location)` は一致するすべてのガウスピークを合計し、結果に `1.0 + hot_zone_scale` を乗算します。

## 生成元データ

`PlayerFactory::load_player_probs()` はプレイヤー生成前に打撃関連の確率データを読み込みます。

| 生成項目 | 生成元 |
| --- | --- |
| `batting_side` | `item_weighted`: `player/batting_side` |
| `batter_type` | `item_weighted`: `player/batter_type` |
| `zone_aptitude` | `item_weighted`: `player/zone_aptitude` |
| `hot_zone_scale` | `normal_param`: `player/batter_info/hot_zone_scale` |
| `batting_eye` | `normal_param`: `player/batter_info/batting_eye` |
| `swing_speed` | `normal_param`: `player/batter_info/swing_speed` |
| `swing_power` | `normal_param`: `player/batter_info/swing_power` |
| `attack_angle` | `normal_param`: `player/batter_info/attack_angle` |
| `bat_control` | `normal_param`: `player/batter_info/bat_control` |
| `consistency` | `normal_param`: `player/batter_info/consistency` |

生成は、生成されたプレイヤーの主ポジションが `P` でない場合のみ行われます。

## 永続化

打撃データは `SqlPlayerRepository::insert_player()` の一部として保存されます。

| テーブル | 用途 |
| --- | --- |
| `batter_info` | 主ポジションが投手でないプレイヤーごとに1行の打撃レコード。主キー: `player_id`。 |

`GameRepository::load_batter()` は打撃レコードを、打席側、打者タイプ、ゾーン適性、ホットゾーンスケール、選球眼、スイング速度、スイングパワー、アタックアングル、バットコントロール、安定度とともに読み込みます。

## 打撃ファクター

`calculate_batting_factor(pitcher, batter, actual_pitch_type, expected_pitch_type, actual_location, expected_location)` は実際の投球と打者の予想を比較します。

| 成分 | 生成元 |
| --- | --- |
| `distance_from_zone_edge` | `actual_location.distance_from_zone_edge()`。 |
| `zone_similarity` | `actual_location.target_zone()` と `expected_location.target_zone()` の類似度。 |
| `pitch_similarity` | 実際と予想の球種スキルのスピン量・スピン角の差。 |
| `zone_aptitude` | `batter.zone_modifier(actual_location)`。 |
| `total_modifier` | `1.0 - zone_aptitude`、`1.0 - zone_similarity`、`1.0 - pitch_similarity` の平均。 |

ゾーン類似度の値:

| ケース | 値 |
| --- | --- |
| 実際と予想がともに `Center` | `0.3` |
| 実際が `Center` で予想が `Center` でない | `0.1` |
| 同じターゲットゾーンのグループ | `0.2` |
| 反対のターゲットゾーンのグループ | `-0.2` |
| 同じ高さまたは同じコース | `0.05` |

球種類似度は `-0.2..0.2` にクランプされます。

## スイング判定

`calculate_swing_factor(approach, count_status, pitch_type, batting_factor)` はカウント、アプローチ、球種、ゾーン、投球読みの各ファクターを組み合わせます。

| 入力 | 挙動 |
| --- | --- |
| `CountStatus::prob()` | カウント依存のスイング傾向。3ボールのカウントは最も見送りが多く、2ストライクのカウントはよりスイング寄りになります。 |
| `PlateApproach::prob()` | 2ストライク前でのみ加算されます。`Aggressive = 0.2`、`Patient = -0.1`、`Take = -5.0`。 |
| 球種 | `FourSeamFastball` は `0.1` を加算し、その他の球種は `-0.05` を加算します。 |
| 打撃ファクター | ゾーン端からの距離、ゾーン類似度、球種類似度、ゾーン適性を加算します。 |

`select_swing_execution(rng, swing_factor)` は `rng.random() < sigmoid(swing_factor)` の場合にスイングし、それ以外は見送ります。

## コンタクト調整

`adapt_to_pitch(offset, bat_control, batting_factor)` はコンタクト評価の前に投球のズレ量をスケーリングします。

| 出力 | 計算式 |
| --- | --- |
| `total_modifier` | `((sigmoid(bat_control) + batting_factor.total_modifier) / 2.0).clamp(0.1, 1.2)` |
| `horizontal_offset_m` | `offset.horizontal_offset_m * total_modifier` |
| `vertical_offset_m` | `offset.vertical_offset_m * total_modifier` |
| `timing_offset_sec` | `offset.timing_offset_sec * total_modifier` |

`calculate_swing_execution_error(rng, batter, actual_location)` は投球位置、ゾーン適性、バットコントロールからバット角度とアタックアングルの誤差を導出します。

| 値 | 挙動 |
| --- | --- |
| `ideal_bat_angle_deg` | `30.0 - location.y * 15.0` を `10.0..60.0` にクランプした値。 |
| `actual_attack_angle_deg` | 動的アタックアングルに打者タイプのアタックアングル補正と5%のランダム変動を加えた値。 |
| `additional_x_m`、`additional_z_m` | 角度誤差と `BAT_BARREL_LENGTH_M = 0.70` から生じる空間的なスイング誤差。 |

## コンタクト結果

`evaluate_swing_contact(batter, offset, swing_execution_error)` はタイミング誤差と空間誤差をバット上に投影します。

| 出力 | 計算式または閾値 |
| --- | --- |
| `timing_impact_x_m` | `batter.swing_speed * offset.timing_offset_sec` |
| `offset_x_m` | 横方向オフセットにスイング実行のX誤差とタイミングの影響を加えた値。 |
| `offset_z_m` | 縦方向オフセットにスイング実行のZ誤差を加えた値。 |
| `thickness_offset_m` | バットの厚み方向に投影したオフセット。 |
| `length_offset_m` | バットの長さ方向に投影したオフセット。 |

コンタクトタイプの閾値:

| コンタクトタイプ | 条件 |
| --- | --- |
| `SwungAndMiss` | `length_offset_m > 0.350` または `thickness_offset_m > 0.070`。 |
| `FoulTip` | `thickness_offset_m > 0.055`。 |
| `WeakContact` | `thickness_offset_m > 0.025`。 |
| `SolidContact` | すべてのオフセットがウィークコンタクト閾値以内。 |
| `Take` | スイングしない場合のデフォルト値。 |

## 打球角度（Launch Angle）

`calculate_launch_angles(contact, batting_side)` はコンタクトのオフセットを縦・横の打球角度に変換します。

| 出力 | 計算式または挙動 |
| --- | --- |
| `vla_deg` | `contact.attack_angle_deg + asin(offset_z / 0.070) * 0.60`。オフセットは `-0.070..0.070` にクランプされます。 |
| `hla_deg` | フェース角成分と跳ね返り成分の合計。左打者では符号が反転します。 |

打球角度の計算で使用される定数:

| 定数 | 値 | 意味 |
| --- | --- | --- |
| `EFFECTIVE_RADIUS_M` | `0.070` | バット半径とボール半径の合計。 |
| `SWING_ARM_RADIUS_M` | `1.10` | スイングの回転半径。 |
| `VLA_REBOUND_FACTOR` | `0.60` | 縦方向の跳ね返りによる偏向の影響度。 |
| `HLA_FACE_FACTOR` | `0.85` | 横方向のフェース角の影響度。 |
| `HLA_REBOUND_FACTOR` | `0.25` | 横方向の跳ね返りの影響度。 |

## 打球初速

`calculate_launch_speed_with_power(contact_result, ball_speed, swing_speed, swing_power)` は打球の初速（秒速メートル）を計算します。

| ステップ | 挙動 |
| --- | --- |
| 最大初速 | `(0.18 * ball_speed) + (c_swing * swing_speed)`。 |
| `c_swing` | `1.12 + 0.16 * effective_power`。effective power は `sigmoid(swing_power)` とコンタクト深度から得られます。 |
| 厚み方向エネルギー | `0.020m` 以内では最大。`0.070m` までに `0.0` へ減衰します。現在のコードは正規化された減衰項に `length_offset_m` を使用しています。 |
| 長さ方向エネルギー | `1.0 - (length_offset_m / 0.35)^2` を `0.0..1.0` にクランプ。 |

前でコンタクトするとスイング伝達が最大5%増加し、深くコンタクトすると最大15%減少します。

## 打球のスピン

`calculate_collision_spin(ball, swing_speed, contact)` は衝突スピンを計算し、残存する投球スピンと合成します。

| 成分 | 挙動 |
| --- | --- |
| コンタクト距離 | `sqrt(length_offset_m^2 + thickness_offset_m^2).min(1.0)`。 |
| 衝突スピン角 | コンタクト位置の角度の反対方向。野球のスピン表記に正規化されます。 |
| 衝突スピン量 | `4000.0 * (swing_speed / 120.0) * contact_distance`。 |
| 投球スピンの保持 | 投球スピンの `20%` がベクトル加算で合成されます。 |

## 軌道

`calculate_batted_ball(batter, ball, contact, stadium)` は初速、打球角度、打球のスピンを計算し、`calculate_trajectory()` を実行して `BattedBall` を作成します。

`calculate_trajectory()` は重力、空気抵抗、マグヌス力、風、地面でのバウンド、フェンス衝突を考慮し、`0.01秒` 刻みで打球をシミュレーションします。

| 定数 | 値 | 意味 |
| --- | --- | --- |
| `IMPACT_HEIGHT_M` | `0.90` | 打球の初期高さ。 |
| `AIR_RESISTANCE_COEFF` | `0.0012` | 簡易空気抵抗係数。 |
| `RESTITUTION_COEFF` | `0.45` | 地面バウンドの反発係数。 |
| `WALL_RESTITUTION` | `0.60` | フェンス衝突の反発係数。 |
| `GROUND_FRICTION` | `0.25` | バウンド後の地面摩擦。 |
| `MAGNUS_COEFF` | `0.0000336` | マグヌス加速度係数。 |

軌道の出力には、初速、打球角度、スピン量、スピン角、最終極座標位置、最大高さ、合計時間、最初のバウンド、任意のフェンス衝突、打球結果が含まれます。

| 打球結果 | 挙動 |
| --- | --- |
| `HomeRun` | バウンド前にフェアゾーンでフェンスを越えます。 |
| `GroundRuleDouble` | 1回以上のバウンドの後にフェンスを越えます。 |
| `Foul` | `FOUL_DEGREE = 45.0` を超えた角度でフェンスを越えます。 |
| `InField` | 打球がフィールド内でプレー可能な場合のデフォルト結果。 |

## ゲームフロー

打席中、`GameState` は以下の順序で打撃解決処理を使用します:

1. 実際の投球と予想された投球を作成する。
2. 投球の変化量とコースバイアスを計算する。
3. `BattingFactor` を計算する。
4. 打者タイプから打席アプローチをサンプリングする。
5. スイングファクターを計算し、`Swing` または `Take` を選択する。
6. 見送りの場合、実際の投球位置をボール、ストライク、死球、暴投として判定する。
7. スイングの場合、投球への適応、コンタクト評価を行い、空振りまたは打球コンタクトを解決する。
