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

## 現行の実行処理とIntent

振る確率・2ストライク防御・能力による誤差低減・追加空間誤差0・0.75時間位置ずれは[打撃ロジック](batting_logic.md)、現行EV・符号付き法線HLA v2・VLA・回転の式は[打球物理](batting_physics.md)を参照。

`Player.batting_character` は `BatterCharacter` として `player_batting_character` に保存する持続的な性格で、BatterInfoおよびBatterTypeとは別。状況別Preferencesと独立したManagerから診断用Intentを生成する。通常打球の置換はまだ行わない。[打撃戦略と監督](batting_strategy.md)を参照。
