# 打撃ロジック

実装: [batting_resolver.rs](../../../src/domain/resolver/batting_resolver.rs)、[GameState::process_count](../../../src/domain/shared/game_state.rs)。2026-11-06時点の通常試合の処理。

## 投球とスイング判断

実投球は投手・捕手の[配球統合](pitch_call_decision.md)で決定する。予想投球は現在も別の `create_pitch` 呼び出しで生成する近似。両者の変化量・到達時間から `PitchDisplacement` を計算する。`calculate_location_bias` のタイミング・水平・垂直の追加バイアスは現在すべて0。

`calculate_batting_factor` は実コースと予想コース、球種の類似度、得意ゾーンから次の係数を作る。

```text
total_modifier = ((1-zone_aptitude) + (1-zone_similarity) + (1-pitch_similarity)) / 3
```

`calculate_swing_decision` のscoreは確率そのものではない。実コースのゾーン外距離を `d = max(distance_from_zone_edge, 0)` とし、以下を加算する。

| 成分 | 現在の処理 |
| --- | --- |
| count | `CountStatus::prob()` |
| approach | 通常は `PlateApproach::prob()`。2ストライクでは `max(prob, 0)` |
| protection | 2ストライクのみ `1.75 * exp(-12*d)` |
| zone | `-2*d` |
| pitch | FourSeamFastballは+0.1、それ以外は−0.05 |
| anticipation | zone_similarity + pitch_similarity。2ストライクで負なら1/4 |
| aptitude | zone_aptitude |

`rng.random() < sigmoid(score)` ならSwing、それ以外はTake。判断用の乱数は1回。
現在の `default_plate_approach` の候補は全BatterTypeでAggressiveのみ。新しいBatterCharacterとは別の仕組み。
見送りは通常のボール・ストライク等の判定へ進む。詳細は[見送り診断](../../swing_decisions.md)。実コースを使う近似であり、独立した知覚コースからの判断ではない。

## 適応と実行誤差

`adapt_to_pitch` は次の係数を水平・垂直・時間のずれに掛ける。

```text
adaptation = clamp(((1-sigmoid(bat_control)) + total_modifier) / 2, 0.1, 1.2)
```

bat_controlが高いほど残留誤差は小さくなる。batting_eyeによるタイミング知覚誤差も `1-sigmoid(batting_eye)` で減衰する。
バット角度の基準は `clamp(30-location.y*15, 10, 60)` 度。角度誤差と動的attack angleは引き続き計算するが、返却する `additional_x_m` / `additional_z_m` は現在0。角度誤差を並進誤差として二重加算しない。

## 接触判定

時間差を変えず、接触位置に換算する成分だけに0.75を掛ける。

```text
timing_impact_x = swing_speed * timing_offset_sec * 0.75
x = horizontal_offset + additional_x + timing_impact_x
z = vertical_offset + additional_z
length_signed = x*cos(theta) + z*sin(theta)
thickness_signed = -x*sin(theta) + z*cos(theta)
```

thetaは実バット傾斜角。絶対値で接触範囲を判定し、符号付き座標も診断へ保存する。

| 判定順 | 条件 |
| --- | --- |
| SwungAndMiss | 長さ方向 > 0.350m、または厚み方向 > 0.070m |
| MarginalContact | 厚み方向 > 0.055m |
| WeakContact | 厚み方向 > 0.025m |
| SolidContact | 上記以外 |

さらに先端寄りの接触は既存の確率的品質低下を受ける。MarginalContactはファウル確定を意味しない。
長さ・厚みのmissは独立した診断フラグで確認する。[接触miss診断](../../contact_miss_diagnostics.md)、[75%位置ずれ](../../timing_position_trial.md)を参照。

## 打球と診断経路

通常の打球は[打球物理](batting_physics.md)のEV・VLA・HLA・スピンから軌道を生成する。Marginal専用のタイミング再抽選は行わない。
別途、Playerの性格と監督からIntentを決め、予測→計画→実行→接触→有限バット衝突応答を診断として記録する。
この応答は通常のスイング／見送り・接触・得点を置換しない。[打撃戦略と監督](batting_strategy.md)、[診断とholdout](diagnostics.md)を参照。
