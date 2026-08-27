# プレイヤーパラメータ

プレイヤーパラメータは主に `src/domain/shared/player.rs` で定義され、`src/domain/player_factory.rs` で生成され、`src/domain/player_service.rs` を経由して読み込まれ、`src/repositories/player_repository.rs` によって永続化されます。

現在の `Player` モデルはネスト構造です:

| フィールド | 型 | 説明 |
| --- | --- | --- |
| `info` | `PlayerInfo` | 識別情報と表示情報。 |
| `offense_skills` | `OffenseSkills` | 打撃と走塁のスキル。 |
| `defense_skills` | `DefenseSkills` | 主ポジションと守備または投球スキル。 |

## PlayerInfo

| パラメータ | 型 | DB保存先 | 説明 |
| --- | --- | --- | --- |
| `id` | `i64` | `player_info.id` | プレイヤー識別子。新しく生成されたプレイヤーは `id = 0` で作成され、保存時のIDはSQLiteの `AUTOINCREMENT` が割り当てます。 |
| `first_name` | `String` | `player_info.first_name` | プレイヤーの名。現在の言語の名前テーブルから生成されます。 |
| `last_name` | `String` | `player_info.last_name` | プレイヤーの姓。現在の言語の名前テーブルから生成されます。 |
| `age` | `u8` | `player_info.age` | プレイヤーの年齢。カテゴリが `player/player_info`、名前が `age` の `gamma_param` から生成されます。 |
| `uniform_number` | `u8` | `player_info.uniform_number` | 背番号。`0..100` から生成されます。現在は一意性が保証されていません。 |

`PlayerInfo::full_name()` はグローバルな `I18nManager` を通じて名前を整形します。

## OffenseSkills

| パラメータ | 型 | DB保存先 | 説明 |
| --- | --- | --- | --- |
| `batter` | `Option<BatterInfo>` | `batter_info` | 打撃パラメータ。ファクトリは現在、投手以外にはこの値を設定し、主ポジションが投手の場合は `None` のままにします。 |
| `running` | `RunningSkills` | `running_skills` | 走塁パラメータ。すべてのプレイヤーで生成・保存されます。 |

## BatterInfo

| パラメータ | 型 | DB保存先 | 説明 |
| --- | --- | --- | --- |
| `batting_side` | `RL` | `batter_info.batting_side` | 打席側。値は `Right`（右打ち）、`Left`（左打ち）。 |
| `batter_type` | `BatterType` | `batter_info.batter_type` | `default_plate_approach()` が使用する打席アプローチのタイプ。 |
| `zone_aptitude` | `ZoneAptitude` | `batter_info.zone_aptitude` | `zone_modifier()` が使用する得意・苦手ゾーンのプロファイル。 |
| `hot_zone_scale` | `f64` | `batter_info.hot_zone_scale` | ゾーン適性モディファイアの強さの倍率。 |
| `batting_eye` | `f64` | `batter_info.batting_eye` | 選球眼・球種判別スキル。`player/batter_info/batting_eye` 配下の `normal_param` から生成されます。 |
| `swing_speed` | `f64` | `batter_info.swing_speed` | スイング速度。`player/batter_info/swing_speed` 配下の `normal_param` から生成されます。 |
| `swing_power` | `f64` | `batter_info.swing_power` | スイングパワー。`player/batter_info/swing_power` 配下の `normal_param` から生成されます。 |
| `attack_angle` | `f64` | `batter_info.attack_angle` | スイングのアタックアングル（度）。`player/batter_info/attack_angle` 配下の `normal_param` から生成されます。 |
| `bat_control` | `f64` | `batter_info.bat_control` | バットコントロールスキル。`player/batter_info/bat_control` 配下の `normal_param` から生成されます。 |
| `consistency` | `f64` | `batter_info.consistency` | コンタクトの安定度。`player/batter_info/consistency` 配下の `normal_param` から生成されます。 |

`BatterInfo::sample_plate_approach(rng)` はプレイヤーの `BatterType` から `PlateApproach` をサンプリングします。`BatterInfo::zone_modifier(location)` は投球位置におけるプレイヤーの `ZoneAptitude` ピークを評価し、結果を `hot_zone_scale` でスケーリングします。

## RunningSkills

| パラメータ | 型 | DB保存先 | 説明 |
| --- | --- | --- | --- |
| `speed` | `f64` | `running_skills.speed` | 基本走力（秒速メートル）。`player/running_skills/running_speed` 配下の `normal_param` から生成されます。 |
| `lead_distance` | `f64` | `running_skills.lead_distance` | 走者のリード距離（メートル）。`player/running_skills/lead_distance` 配下の `normal_param` から生成されます。 |
| `start_reaction` | `f64` | `running_skills.start_reaction` | 走者のスタート反応。`player/running_skills/start_reaction` 配下の `normal_param` から生成されます。 |

## DefenseSkills

| パラメータ | 型 | DB保存先 | 説明 |
| --- | --- | --- | --- |
| `position` | `Position` | `defense_skills.position` | 主守備ポジション。 |
| `pitcher` | `Option<PitcherInfo>` | `pitcher_info`、`pitch_skill`、`fielder_info` | 投手パラメータと投手の守備情報。プレイヤーが `FielderType::Pitcher` を持つ場合に設定されます。 |
| `catcher` | `Option<CatcherInfo>` | `fielder_info` | 捕手の守備情報。プレイヤーが `FielderType::Catcher` を持つ場合に設定されます。 |
| `middle_infielder` | `Option<FielderInfo>` | `fielder_info` | 二遊間（中堅内野手）の守備情報。`SB` と `SS` タイプのスキルで設定されます。 |
| `corner_infielder` | `Option<FielderInfo>` | `fielder_info` | コーナー内野手の守備情報。`FB` と `TB` タイプのスキルで設定されます。 |
| `outfielder` | `Option<FielderInfo>` | `fielder_info` | 外野手の守備情報。`LF`、`CF`、`RF` タイプのスキルで設定されます。 |

`DefenseSkills::new(position)` は主ポジションを設定し、すべての任意スキルグループを `None` に初期化します。

## FielderInfo

| パラメータ | 型 | DB保存先 | 説明 |
| --- | --- | --- | --- |
| `fielder_type` | `FielderType` | `fielder_info.fielder_type` | 守備スキルグループ。`DH` はポジションであり、`FielderType` ではありません。 |
| `throw_speed` | `f64` | `fielder_info.throw_speed` | 送球速度（秒速メートル）。 |
| `running_speed` | `f64` | `fielder_info.running_speed` | 守備時の走力（秒速メートル）。 |
| `reaction` | `f64` | `fielder_info.reaction` | 守備の反応時間（秒）。小さいほど良い。 |
| `prep_time` | `f64` | `fielder_info.prep_time` | 送球の準備・持ち替え時間（秒）。小さいほど良い。 |
| `catching` | `f64` | `fielder_info.catching` | 捕球スキル。 |
| `reach_height` | `f64` | `fielder_info.reach_height` | 縦方向の到達高。 |
| `reach_range` | `f64` | `fielder_info.reach_range` | 横方向の到達範囲。現在は `1.0` として生成されます。 |

`FielderInfo` の数値の大半は `player/fielder_info` 配下の `normal_param` から生成されます。`reach_range` は現在 `PlayerFactory::assign_fielder_info()` で `1.0` に固定されています。

## PitcherInfo

| パラメータ | 型 | DB保存先 | 説明 |
| --- | --- | --- | --- |
| `height` | `f64` | `pitcher_info.height` | 投手の身長。 |
| `extension` | `f64` | `pitcher_info.extension` | 投球のリーチ。リリースポイント計算では `1.2` と `2.3` の間にクランプされます。 |
| `throw_side` | `RL` | `pitcher_info.throw_side` | 投手の投球側。値は `Right`（右投げ）、`Left`（左投げ）。 |
| `arm_slot` | `ArmSlot` | `pitcher_info.arm_slot` | 投球フォームのアームスロット。 |
| `pitcher_style` | `PitcherStyle` | `pitcher_info.pitcher_style` | 球種の選択に使用される投手タイプ。 |
| `velocity` | `f64` | `pitcher_info.velocity` | 投手レベルの球速。 |
| `spin_rate` | `f64` | `pitcher_info.spin_rate` | 投手レベルのスピン量。 |
| `control` | `f64` | `pitcher_info.control` | 投手レベルの制球力。 |
| `stamina` | `f64` | `pitcher_info.stamina` | 投手レベルのスタミナ。 |
| `injury_proneness` | `f64` | `pitcher_info.injury_proneness` | 投手レベルの故障しやすさ。 |
| `clutch` | `f64` | `pitcher_info.clutch` | 投手レベルの勝負強さ。 |
| `hpp` | `f64` | `pitcher_info.hpp` | ホーム・アウェイ分割の値。 |
| `platoon_splitting` | `f64` | `pitcher_info.platoon_splitting` | 左右別（プラトーン）分割の値。 |
| `delivery_motion_time` | `f64` | `pitcher_info.delivery_motion_time` | 投球モーション時間。 |
| `consistency` | `f64` | `pitcher_info.consistency` | 投手レベルの安定度。 |
| `pitch_skills` | `Vec<PitchSkill>` | `pitch_skill` | 球種別のスキル値と使用頻度。 |
| `fielder_info` | `FielderInfo` | `fielder_info` | `FielderType::Pitcher` として保存される投手の守備情報。 |

PitcherInfo は以下のメソッドも導出します:

| メソッド | 挙動 |
| --- | --- |
| `calculate_release_point()` | 身長、リーチ、アームスロット、投球側からリリース `(x, y, z)` を計算します。 |
| `base_spin_angle()` | アームスロットに対応する直球の基本スピン角を返します。左投手では左右反転されます。 |
| `pitch_skill_distribution()` | 球種スキルの `usage` 値に `softmax` を適用します。 |
| `pitch_calling_distribution()` | 球種分布とデフォルトのコース分布を組み合わせます。 |
| `sample_pitch_type(rng)` | 球種分布に基づいて `PitchSkill` をサンプリングします。 |

## PitchSkill

| パラメータ | 型 | DB保存先 | 説明 |
| --- | --- | --- | --- |
| `pitch_type` | `PitchType` | `pitch_skill.pitch_type` | 球種。 |
| `velocity` | `f64` | `pitch_skill.velocity` | 球種別の球速。 |
| `control` | `f64` | `pitch_skill.control` | 球種別の制球力。 |
| `stamina` | `f64` | `pitch_skill.stamina` | 球種別のスタミナ。 |
| `injury_proneness` | `f64` | `pitch_skill.injury_proneness` | 球種別の故障しやすさ。 |
| `spin_rate` | `f64` | `pitch_skill.spin_rate` | 球のスピン量。 |
| `spin_angle` | `f64` | `pitch_skill.spin_angle` | 球のスピン角。 |
| `spin_efficiency` | `f64` | `pitch_skill.spin_efficiency` | 球のスピン効率。 |
| `usage` | `f64` | `pitch_skill.usage` | 球種の使用頻度スコア。実行時の球種分布はこれらの値の softmax に基づきます。 |

球種スキルは `pitch_type/<PitchType>` 配下の `normal_param` 行から生成されます。利用可能な球種は `pitcher_style/<PitcherStyle>` 配下の `item_weighted` 行から選択されます。

## 列挙型の値

### RL

| 値 | 意味 |
| --- | --- |
| `Right` | 右投げまたは右打ち。 |
| `Left` | 左投げまたは左打ち。 |

### Position

| 値 | 意味 |
| --- | --- |
| `P` | 投手 |
| `C` | 捕手 |
| `FB` | 一塁 |
| `SB` | 二塁 |
| `TB` | 三塁 |
| `SS` | 遊撃手 |
| `LF` | 左翼 |
| `CF` | 中堅 |
| `RF` | 右翼 |
| `DH` | 指名打者 |

### FielderType

| 値 | 意味 |
| --- | --- |
| `Outfielder` | 外野手スキルグループ。 |
| `MiddleInfielder` | 二遊間（中堅内野手）スキルグループ。 |
| `CornerInfielder` | コーナー内野手スキルグループ。 |
| `Pitcher` | 投手スキルグループ。 |
| `Catcher` | 捕手スキルグループ。 |

### BatterType

| 値 | 意味 |
| --- | --- |
| `AggressiveFreeSwinger` | 積極的で感覚的な打者。 |
| `ClassicAnalyst` | 慎重で理論派の打者。 |
| `GameManager` | 状況に応じて対応する打者。 |
| `ClutchHunter` | ハイリスクで長打志向の打者。 |

### ZoneAptitude

| 値 | 意味 |
| --- | --- |
| `Balanced` | 特定の偏りのない均等なゾーンプロファイル。 |
| `InsideDominant` | 内角の球に強い。 |
| `OutsideDominant` | 外角の球に強い。 |
| `LowBaller` | 低めの球に強い。 |
| `HighBaller` | 高めの球に強い。 |
| `DiagonalCross` | 対角線上のゾーンパターンに強い。 |

### ArmSlot

| 値 | 右投手の基本スピン角 |
| --- | --- |
| `Overhand` | `25.0` 度 |
| `ThreeQuarter` | `55.0` 度 |
| `Sidearm` | `85.0` 度 |
| `Submarine` | `115.0` 度 |

左投手は `(360.0 - base_deg) % 360.0` で基本スピン角を左右反転します。

### PitcherStyle

| 値 | 意味 |
| --- | --- |
| `PowerPitcher` | パワーピッチャー。 |
| `FinessePitcher` | フィネスピッチャー。 |
| `BalancedPitcher` | バランス型ピッチャー。 |

### PitchType

| 値 | 意味 |
| --- | --- |
| `FourSeamFastball` | フォーシーム直球 |
| `Cutter` | カッター |
| `Curveball` | カーブ |
| `Slider` | スライダー |
| `Changeup` | チェンジアップ |
| `Splitter` | スプリッター |

## 生成メモ

`PlayerFactory::load_player_probs()` は生成前にすべての確率入力データを読み込みます:

| ローダー | 生成元 |
| --- | --- |
| `load_player_info_probs()` | `gamma_param`: `player/player_info/age` |
| `load_running_skill_probs()` | `normal_param`: `player/running_skills/*` |
| `load_batter_info_probs()` | `item_weighted`: `player/batting_side`、`player/batter_type`、`player/zone_aptitude`; `normal_param`: `player/batter_info/*` |
| `load_fielder_info_probs()` | `item_weighted`: `player/fielder_type`; `normal_param`: `player/fielder_info/*` |
| `load_pitcher_info_prob()` | `item_weighted`: `pitcher_info/throw_side`、`pitcher_info/arm_slot`、`pitcher_info/pitcher_style`; `normal_param`: `player/pitcher_info/*` |
| `load_pitch_type_prob()` | `item_weighted`: `pitcher_style/<PitcherStyle>` |
| `load_pitch_skill_probs()` | `normal_param`: `pitch_type/<PitchType>/*` |

主ポジションは最初に選択された `FielderType` から割り当てられます:

| 野手タイプ | 主ポジションの重み |
| --- | --- |
| `Outfielder` | `RF` 0.32、`CF` 0.32、`LF` 0.32、`DH` 0.04 |
| `MiddleInfielder` | `SS` 0.48、`SB` 0.52 |
| `CornerInfielder` | `FB` 0.5、`TB` 0.4、`DH` 0.1 |
| `Pitcher` | 常に `P` |
| `Catcher` | 常に `C` |

ファクトリは `multiple_fielder_type/<FielderType>` から追加の守備スキルグループを追加することがあります。生成された各プレイヤーは、その主ポジションの選手が最も少ないチームに割り当てられます。その探索が `NotFound` で失敗した場合は、ランダムなチームが選択されます。

## コンストラクタとヘルパー

| コンストラクタまたはヘルパー | 挙動 |
| --- | --- |
| `PlayerInfo::new_unsaved(first_name, last_name, age, uniform_number)` | `id = 0` の未保存情報を作成します。 |
| `PlayerInfo::new(id, first_name, last_name, age, uniform_number)` | すべての項目が設定されたプレイヤー情報を作成します。 |
| `PlayerInfo::new_min(id, first_name, last_name)` | `age = 0` と `uniform_number = 0` の最小構成のプレイヤー情報を作成します。 |
| `Player::from_player_info(info)` | 指定された情報で `Player` を作成します。打者・投手情報はなく、走塁スキルはゼロ、`DefenseSkills::new(Position::DH)` を使用します。 |
| `Player::is(id)` | プレイヤーのidが `id` と一致するかを返します。 |
| `Player::full_name()` | `PlayerInfo::full_name()` を返します。 |
| `Player::batter()` | 打者情報を返します。存在しない場合は `GameError::BatterInfo` を返します。 |
| `Player::runner()` | 走塁スキルを返します。 |
| `Player::pitcher()` | 投手情報を返します。存在しない場合は `GameError::PitcherInfo` を返します。 |
| `Player::catcher()` | 捕手情報を返します。存在しない場合は `GameError::PitcherInfo` を返します。 |
| `Player::fielder()` | プレイヤーの主ポジションの守備情報を返します。存在しない場合は `GameError::FielderInfo` を返します。 |
| `DefenseSkills::new(position)` | 主ポジション用の守備スキルを作成し、すべての任意スキルグループを `None` に設定します。 |
| `FielderInfo::new_pitcher()` | ゼロ初期化された投手用守備情報を作成します。 |
| `CatcherInfo::from_fielder_info(fielder_info)` | 野手情報を捕手情報としてラップします。 |
