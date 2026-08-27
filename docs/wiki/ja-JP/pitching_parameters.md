# 投球パラメータ

投球関連のパラメータは `src/domain/shared/player.rs` で定義され、`src/domain/player_factory.rs` で生成され、`src/domain/player_service.rs` を経由して読み込まれ、`src/repositories/player_repository.rs` によって永続化され、`src/domain/resolver/pitching_resolver.rs` で使用されます。

投球データは現在、`FielderType::Pitcher` を持つプレイヤーに対してのみ生成・永続化されます。

## PitcherInfo

`PitcherInfo` は投手レベルの属性、持ち球（球種リスト）、投手の守備情報を保持します。

| パラメータ | 型 | DB保存先 | 説明 |
| --- | --- | --- | --- |
| `height` | `f64` | `pitcher_info.height` | 投手の身長。リリース高の算出に使用されます。 |
| `extension` | `f64` | `pitcher_info.extension` | 投球のリーチ（前傾によるリリース位置の前進距離）。リリースポイント計算では `PITCH_EXTENSION_MIN = 1.2` と `PITCH_EXTENSION_MAX = 2.3` の間にクランプされます。 |
| `throw_side` | `RL` | `pitcher_info.throw_side` | 投球側。値は `Right`（右投げ）、`Left`（左投げ）。 |
| `arm_slot` | `ArmSlot` | `pitcher_info.arm_slot` | 投球フォームのアームスロット。リリースポイントと基本スピン角の算出に使用されます。 |
| `pitcher_style` | `PitcherStyle` | `pitcher_info.pitcher_style` | 球種の選択に使用される投手タイプ。 |
| `velocity` | `f64` | `pitcher_info.velocity` | 投手レベルの球速（倍率・基準値）。 |
| `spin_rate` | `f64` | `pitcher_info.spin_rate` | 投手レベルのスピン量の基準値。 |
| `control` | `f64` | `pitcher_info.control` | 投手レベルの制球力。 |
| `stamina` | `f64` | `pitcher_info.stamina` | 投手レベルのスタミナ。 |
| `injury_proneness` | `f64` | `pitcher_info.injury_proneness` | 投手レベルの故障しやすさ。 |
| `clutch` | `f64` | `pitcher_info.clutch` | 投手レベルの勝負強さ。現在のファクトリコードでは `pitcher_info_probs.injury_proneness` から生成されます。 |
| `hpp` | `f64` | `pitcher_info.hpp` | ホーム・アウェイ分割（本拠地・敵地の成績差）の値。 |
| `platoon_splitting` | `f64` | `pitcher_info.platoon_splitting` | 左右別（プラトーン）成績差の値。 |
| `delivery_motion_time` | `f64` | `pitcher_info.delivery_motion_time` | 投球モーション時間。守備・走塁の解決処理でも使用されます。 |
| `consistency` | `f64` | `pitcher_info.consistency` | hanging pitch（失投）効果のサンプリングに使用される投手レベルの安定度。 |
| `pitch_skills` | `Vec<PitchSkill>` | `pitch_skill` | 球種ごとの値と使用頻度スコア。 |
| `fielder_info` | `FielderInfo` | `fielder_info` | `FielderType::Pitcher` として保存される投手の守備情報。 |

## PitchSkill

`PitchSkill` は投手の持ち球である各球種の球種別パラメータを保持します。

| パラメータ | 型 | DB保存先 | 説明 |
| --- | --- | --- | --- |
| `pitch_type` | `PitchType` | `pitch_skill.pitch_type` | 球種。`player_id` と合わせて主キーを構成します。 |
| `velocity` | `f64` | `pitch_skill.velocity` | 球種別の球速係数。`create_pitch()` は `pitcher.velocity * pitch_skill.velocity` として乗算します。 |
| `control` | `f64` | `pitch_skill.control` | 球種別の制球力。 |
| `stamina` | `f64` | `pitch_skill.stamina` | 球種別のスタミナ。 |
| `injury_proneness` | `f64` | `pitch_skill.injury_proneness` | 球種別の故障しやすさ。 |
| `spin_rate` | `f64` | `pitch_skill.spin_rate` | ランダム変動と球速補正を受ける前のスピン量。 |
| `spin_angle` | `f64` | `pitch_skill.spin_angle` | 投手の基本スピン角からの球種別オフセット。 |
| `spin_efficiency` | `f64` | `pitch_skill.spin_efficiency` | マグヌス変化に寄与するスピンの割合。 |
| `usage` | `f64` | `pitch_skill.usage` | 使用頻度スコア。球種選択では全 usage 値に `softmax` を適用します。 |

## 球種

利用可能な球種は `PitchType` のバリアントです:

| 値 | 意味 |
| --- | --- |
| `FourSeamFastball` | フォーシーム直球 |
| `Cutter` | カッター |
| `Curveball` | カーブ |
| `Slider` | スライダー |
| `Changeup` | チェンジアップ |
| `Splitter` | スプリッター |

## アームスロット

`ArmSlot` は投手の基本スピン角とリリースポイントを決定します。

| 値 | 右投手の基本スピン角 | リリース高係数 | リリース横距離 |
| --- | --- | --- | --- |
| `Overhand` | `25.0` 度 | `1.05 * height` | `0.35` |
| `ThreeQuarter` | `55.0` 度 | `0.95 * height` | `0.55` |
| `Sidearm` | `85.0` 度 | `0.70 * height` | `0.85` |
| `Submarine` | `115.0` 度 | `0.40 * height` | `0.60` |

左投手は `(360.0 - base_deg) % 360.0` で基本スピン角を左右反転し、リリースの `x` 座標の符号を反転させます。

## 投手タイプ（PitcherStyle）

`PitcherStyle` は投手が投げられる球種の選択に使用されます。

| 値 | 意味 |
| --- | --- |
| `PowerPitcher` | パワーピッチャー。 |
| `FinessePitcher` | フィネスピッチャー（制球派）。 |
| `BalancedPitcher` | バランス型ピッチャー。 |

球種の重みは `PlayerService::load_pitch_type_prob()` によってスタイルごとに `item_weighted` から読み込まれます。

## 生成元データ

`PlayerFactory::load_player_probs()` はプレイヤー生成前に投球関連の確率データを読み込みます。

| 生成項目 | 生成元 |
| --- | --- |
| `throw_side` | `item_weighted`: `pitcher_info/throw_side` |
| `arm_slot` | `item_weighted`: `pitcher_info/arm_slot` |
| `pitcher_style` | `item_weighted`: `pitcher_info/pitcher_style` |
| `height` | `normal_param`: `player/pitcher_info/height` |
| `extension` | `normal_param`: `player/pitcher_info/extension` |
| `velocity` | `normal_param`: `player/pitcher_info/velocity` |
| `spin_rate` | `normal_param`: `player/pitcher_info/spin_rate` |
| `control` | `normal_param`: `player/pitcher_info/control` |
| `stamina` | `normal_param`: `player/pitcher_info/stamina` |
| `injury_proneness` | `normal_param`: `player/pitcher_info/injury_proneness` |
| `clutch` | `normal_param`: `player/pitcher_info/clutch` から読み込みますが、現在のファクトリでは `injury_proneness` を使用しています。 |
| `hpp` | `normal_param`: `player/pitcher_info/hpp` |
| `platoon_splitting` | `normal_param`: `player/pitcher_info/platoon_splitting` |
| `delivery_motion_time` | `normal_param`: `player/pitcher_info/delivery_motion_time` |
| `consistency` | `normal_param`: `player/pitcher_info/consistency` |
| 持ち球 | `item_weighted`: `pitcher_style/<PitcherStyle>` |
| 球種別パラメータ | `normal_param`: `pitch_type/<PitchType>/{velocity,control,stamina,injury_proneness,spin_rate,spin_angle,spin_efficiency,usage}` |

生成は、生成されたプレイヤーが `FielderType::Pitcher` を含む場合のみ行われます。主ポジションが投手のプレイヤーは `offense_skills.batter = None` となります。

## 永続化

投球データは `SqlPlayerRepository::insert_player()` の一部として保存されます。

| テーブル | 用途 |
| --- | --- |
| `pitcher_info` | 投手プレイヤーごとに1行の投手レベルレコード。主キー: `player_id`。 |
| `pitch_skill` | プレイヤーの球種ごとに1行。主キー: `(player_id, pitch_type)`。 |
| `fielder_info` | `fielder_type = Pitcher` の投手守備レコード。 |

`GameRepository::load_pitcher_info()` は投手レコードを読み込み、`pitch_skill` から `pitch_skills`、`fielder_info` から投手の `fielder_info` を結合します。

## リリースポイント

`PitcherInfo::calculate_release_point(rng)` はリリース座標を導出します:

| 座標 | 計算式 |
| --- | --- |
| `x` | アームスロット別の横距離。左投手では符号を反転。 |
| `y` | `18.44 - (extension * rng.normal_factor_std_1_percent()).clamp(1.2, 2.3)` |
| `z` | `height * arm_slot_height_factor` |

## 投球生成

`calculate_hanging_pitch_effect(rng, pitcher)` は、`rng.random() < pitcher.consistency` の場合に共通の投球効果を返します。それ以外の場合は各球が通常の1%係数を使用します。`create_pitch(rng, pitcher, hanging_pitch_effect)` は投手パラメータを `PitchedBall` に変換します。

| 出力 | 計算式または生成元 |
| --- | --- |
| `pitch_type` | 球種の使用頻度とデフォルトのコース分布から `PitchCall` の一部としてサンプリングされます。 |
| `speed` | 秒速メートルで `pitcher.velocity * pitch_skill.velocity * pitch_effect`。 |
| `spin_rate` | `pitcher.spin_rate * pitch_skill.spin_rate * pitch_effect.max(1.0) * (speed / BASE_FOUR_SEAM_SPEED)`。 |
| `spin_angle` | `pitch_skill.spin_angle * pitch_effect` を `-180.0..180.0` にクランプし、右投手では基本スピン角に加算、左投手では減算した後、`0..360` に正規化します。 |
| `spin_efficiency` | `pitch_skill.spin_efficiency` |
| `release_point` | `pitcher.calculate_release_point(rng)` |
| `flight_time` | `release_point.y / (speed * 0.95)` |
| `aim_zone` | サンプリングされた `PitchCall` の狙いゾーン。 |
| `aim_location` | 投球コールのターゲットゾーンとマージンから導出されます。 |
| `actual_location` | `pitcher.control` とターゲットゾーンのサイズに基づいて `aim_location` の周囲でサンプリングされます。 |

投球解決で使用される定数:

| 定数 | 値 | 意味 |
| --- | --- | --- |
| `BASE_FOUR_SEAM_SPEED` | `40.833` | スピン量の球速補正に使用する基準球速（秒速メートル）。 |
| `PITCH_OFFSET_DECISION_RATIO` | `0.6` | 投球オフセット計算で使用されるスイング判断地点の比率。 |
| `AIR_DRAG_FACTOR` | `0.95` | 飛行時間の計算に使用する平均球速係数。 |
| `MAGNUS_COEFF` | `0.0000336` | `PitchedBall` の変化量ヘルパーが使用するマグヌス加速度係数。 |

## 投球変化

`calculate_ball_movement(ball)` はスピンと重力から物理的な変化量を推定します:

| 成分 | 挙動 |
| --- | --- |
| 横方向 | `0.5 * ball.get_side_accel() * flight_time^2`。 |
| 縦方向 | `0.5 * (ball.get_vertical_accel() - GRAVITY) * flight_time^2`。 |

`calculate_location_bias(location)` は実際の投球位置からタイミングと空間的なバイアスを導出します。内角の球はタイミングを遅めに、外角の球は早めに偏らせます。高め・低めの球は縦方向のコンタクトに偏りを与えます。

`calculate_pitch_offset(rng, pitched_ball, expected_ball, matchup, location_bias, batting_eye)` は飛行時間の最後の40%における実際の投球変化と予想された投球変化を比較します。晩期変化（ラテブレイク）、利き腕に基づくクロスファイア知覚、リリース位置の横幅知覚、コースバイアス、タイミング誤差を組み合わせます。

| 対戦カード | クロスファイア倍率 |
| --- | --- |
| 左投手 vs 右打者 | `1.30` |
| 右投手 vs 左打者 | `1.05` |
| 同サイドその他の対戦 | `1.00` |

`calculate_timing_offset(rng, pitched_ball, expected_ball, batting_eye)` は、リリースポイントのノイズ、打者の選球眼（知覚）、球速係数を適用した上で、実際の飛行時間と予想された飛行時間を比較します。

## 球種・コースの選択（Pitch Calling）

`PitcherInfo::pitch_skill_distribution()` は球種スキルの使用頻度に `softmax` を適用します。`PitcherInfo::pitch_calling_distribution()` は以下を組み合わせます:

| 分布 | 生成元 |
| --- | --- |
| 球種 | `PitcherInfo::pitch_skill_distribution()` |
| コース | `default_location_distribution()` |

現在のデフォルトのコース分布は次のとおりです:

| TargetZone | 重み |
| --- | --- |
| `LowOutside` | `0.8` |
| `HighInside` | `0.2` |

現在、生成されるすべての `PitchCall` は `Margin::Edge` を使用します。利用可能なターゲットゾーンは `Center`（中央）、`LowInside`（低め内角）、`LowOutside`（低め外角）、`HighInside`（高め内角）、`HighOutside`（高め外角）です。
