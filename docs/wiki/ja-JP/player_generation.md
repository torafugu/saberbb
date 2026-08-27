# プレイヤー生成

プレイヤー生成は `src/domain/player_factory.rs` で実装されています。`src/domain/player_service.rs` が確率データを読み込み、`PlayerRepository` を通じて生成されたプレイヤーを保存します。

`src/main.rs` のCLIプレイヤー生成モードは `PlayerFactory` を作成し、`load_player_probs()` を一度呼び出した後、要求された各プレイヤーに対して `generate_and_save_player()` を呼び出します。

## 生成フロー

`PlayerFactory::generate_and_save_player()` は各プレイヤーに対して次の処理を繰り返します:

1. `generate_player()` で `Player` を生成する。
2. `assign_team(&player)` でチームを割り当てる。
3. `PlayerService::save_player(team.id, &player)` でプレイヤーを保存する。

`generate_player()` は以下の順序でプレイヤーを構築します:

1. ランダムなローカライズ名を読み込み、`PlayerInfo` を生成する。
2. `player/fielder_type` から最初の `FielderType` を選択する。
3. その野手タイプを主ポジションの `Position` に変換する。
4. `multiple_fielder_type/<FielderType>` から追加の野手タイプを任意で追加する。
5. `DefenseSkills` を生成し、対応する守備・捕球・投球情報を紐付ける。
6. 投手以外の主ポジションには `BatterInfo` を生成する（主ポジションが投手の場合は現在 `None`）。
7. 全プレイヤーに `RunningSkills` を生成する。
8. ネストされた `Player { info, offense_skills, defense_skills }` を返す。

## 確率データの読み込み

`load_player_probs()` は以下の確率キャッシュを読み込みます:

| キャッシュ | サービス側ローダー | リポジトリの生成元 |
| --- | --- | --- |
| `player_info_probs` | `load_player_info_probs()` | `gamma_param`: `player/player_info/age` |
| `running_skill_probs` | `load_running_skill_probs()` | `normal_param`: `player/running_skills/*` |
| `batter_info_probs` | `load_batter_info_probs()` | `item_weighted`: `player/batting_side`、`player/batter_type`、`player/zone_aptitude`; `normal_param`: `player/batter_info/*` |
| `fielder_info_probs` | `load_fielder_info_probs()` | `item_weighted`: `player/fielder_type`; `normal_param`: `player/fielder_info/*` |
| `pitcher_info_probs` | `load_pitcher_info_prob()` | `item_weighted`: `pitcher_info/throw_side`、`pitcher_info/arm_slot`、`pitcher_info/pitcher_style`; `normal_param`: `player/pitcher_info/*` |
| `pitch_type_map` | `load_pitch_type_prob()` | `item_weighted`: `pitcher_style/<PitcherStyle>` |
| `pitch_skill_map` | `load_pitch_skill_probs()` | `normal_param`: `pitch_type/<PitchType>/*` |

`load_player_probs()` は現在 `player_info_probs` を2回読み込みます。2回目の読み込みが同じ生成元データで1回目を上書きします。

## ランダムヘルパー

| ヘルパー | 挙動 |
| --- | --- |
| `RandomProvider::gamma(param)` | `Gamma(shape, scale) + offset` をサンプリングします。年齢に使用され、`u8` に丸められます。 |
| `RandomProvider::normal(param)` | `Normal(mean, std_dev)` をサンプリングし、任意の歪みを適用した後、`* coefficient + offset` します。 |
| `RandomProvider::gen_range(0, 100)` | 背番号を生成します。現在の `RealRng` 実装は両端を含む範囲を使用します。 |
| `choose_item_weighted(items)` | アイテムの重みに基づいて1つのアイテムを選択します。重みの合計は正である必要がありますが、`1.0` に一致する必要はありません。 |
| `choose_item_if_exists(items)` | 各アイテムを独立に判定し、`rng.random() < item.weight` の場合に採用します。 |

## PlayerInfo

| フィールド | 生成方法 |
| --- | --- |
| `id` | `PlayerInfo::new_unsaved()` は `id = 0` を設定します。保存時のIDはSQLiteが割り当てます。 |
| `first_name`、`last_name` | `PlayerService::load_random_name()` がアクティブなDB言語の `first_names` と `last_names` を問い合わせます。 |
| `age` | `rng.gamma(player_info_probs.age).round() as u8`。 |
| `uniform_number` | `rng.gen_range(0, 100) as u8`。現在は一意性が保証されていません。 |

## ポジションと野手タイプ

最初の野手タイプはカテゴリ `player/fielder_type` の `item_weighted` 行から選択されます。

`migrations/dml/item_weighted_sample.sql` のサンプル重み:

| 野手タイプ | サンプル重み |
| --- | --- |
| `Outfielder` | `0.24` |
| `MiddleInfielder` | `0.12` |
| `CornerInfielder` | `0.12` |
| `Pitcher` | `0.42` |
| `Catcher` | `0.10` |

その後、主ポジションは `PlayerFactory::assign_position()` のハードコードされた重みで割り当てられます:

| 野手タイプ | 主ポジションの重み |
| --- | --- |
| `Outfielder` | `RF` 0.32、`CF` 0.32、`LF` 0.32、`DH` 0.04 |
| `MiddleInfielder` | `SS` 0.48、`SB` 0.52 |
| `CornerInfielder` | `FB` 0.5、`TB` 0.4、`DH` 0.1 |
| `Pitcher` | 常に `P` |
| `Catcher` | 常に `C` |

追加の野手タイプは `multiple_fielder_type/<最初のFielderType>` から読み込まれ、`choose_item_if_exists()` で独立に判定されます。最初に選択された野手タイプは、任意の追加野手タイプの後に常に追加されます。

## 守備データの生成

`DefenseSkills::new(primary_position)` は主ポジションを持ち、すべての任意グループが `None` の状態で開始します。

選択された各野手タイプについて:

| 野手タイプ | 生成される情報 | 保存先フィールド |
| --- | --- | --- |
| `Outfielder` | `FielderInfo` | `defense_skills.outfielder` |
| `MiddleInfielder` | `FielderInfo` | `defense_skills.middle_infielder` |
| `CornerInfielder` | `FielderInfo` | `defense_skills.corner_infielder` |
| `Pitcher` | 投手用 `FielderInfo` を内包した `PitcherInfo` | `defense_skills.pitcher` |
| `Catcher` | `FielderInfo` をラップした `CatcherInfo` | `defense_skills.catcher` |

`FielderInfo` の値は `player/fielder_info` 配下の `normal_param` から生成されます:

| フィールド | 生成元 |
| --- | --- |
| `throw_speed` | `player/fielder_info/throw_speed` |
| `running_speed` | `player/fielder_info/running_speed` |
| `reaction` | `player/fielder_info/reaction` |
| `prep_time` | `player/fielder_info/prep_time` |
| `catching` | `player/fielder_info/catching` |
| `reach_height` | `player/fielder_info/reach_height` |
| `reach_range` | `assign_fielder_info()` で `1.0` に固定。 |

## 打者データの生成

ファクトリは主ポジションが `P` でない場合にのみ `BatterInfo` を生成します。

| フィールド | 生成元 |
| --- | --- |
| `batting_side` | `player/batting_side` からの重み付き選択。 |
| `batter_type` | `player/batter_type` からの重み付き選択。 |
| `zone_aptitude` | `player/zone_aptitude` からの重み付き選択。 |
| `hot_zone_scale` | `normal_param`: `player/batter_info/hot_zone_scale`。 |
| `batting_eye` | `normal_param`: `player/batter_info/batting_eye`。 |
| `swing_speed` | `normal_param`: `player/batter_info/swing_speed`。 |
| `swing_power` | `normal_param`: `player/batter_info/swing_power`。 |
| `attack_angle` | `normal_param`: `player/batter_info/attack_angle`。 |
| `bat_control` | `normal_param`: `player/batter_info/bat_control`。 |
| `consistency` | `normal_param`: `player/batter_info/consistency`。 |

実行時、`batter_type` は `default_plate_approach()` を通じて `BatterInfo::sample_plate_approach(rng)` を駆動します。`zone_aptitude` と `hot_zone_scale` は `zone_aptitude_peaks()` を通じて `BatterInfo::zone_modifier(location)` を駆動します。

## 走塁能力の生成

生成されたすべてのプレイヤーは `RunningSkills` を受け取ります。

| フィールド | 生成元 |
| --- | --- |
| `speed` | `normal_param`: `player/running_skills/running_speed` |
| `lead_distance` | `normal_param`: `player/running_skills/lead_distance` |
| `start_reaction` | `normal_param`: `player/running_skills/start_reaction` |

## 投手データの生成

投手データは、選択された野手タイプに `FielderType::Pitcher` が含まれる場合に生成されます。

| フィールド | 生成元 |
| --- | --- |
| `throw_side` | `pitcher_info/throw_side` からの重み付き選択。 |
| `arm_slot` | `pitcher_info/arm_slot` からの重み付き選択。 |
| `pitcher_style` | `pitcher_info/pitcher_style` からの重み付き選択。 |
| `height` | `normal_param`: `player/pitcher_info/height`。 |
| `extension` | `normal_param`: `player/pitcher_info/extension`。 |
| `velocity` | `normal_param`: `player/pitcher_info/velocity`。 |
| `spin_rate` | `normal_param`: `player/pitcher_info/spin_rate`。 |
| `control` | `normal_param`: `player/pitcher_info/control`。 |
| `stamina` | `normal_param`: `player/pitcher_info/stamina`。 |
| `injury_proneness` | `normal_param`: `player/pitcher_info/injury_proneness`。 |
| `clutch` | 現在のファクトリコードは `player/pitcher_info/injury_proneness` を使用します。`player/pitcher_info/clutch` は読み込まれますが、ここでは使用されません。 |
| `hpp` | `normal_param`: `player/pitcher_info/hpp`。 |
| `platoon_splitting` | `normal_param`: `player/pitcher_info/platoon_splitting`。 |
| `delivery_motion_time` | `normal_param`: `player/pitcher_info/delivery_motion_time`。 |
| `consistency` | `normal_param`: `player/pitcher_info/consistency`。 |
| `pitch_skills` | 投手のスタイルに応じて選択された球種から生成されます。 |
| `fielder_info` | `FielderType::Pitcher` 用に生成された `FielderInfo`。 |

持ち球は `assign_pitch_skill()` で選択されます:

1. 選択された `PitcherStyle` の球種確率を `pitcher_style/<PitcherStyle>` から読み込む。
2. 各球種を `choose_item_if_exists()` で独立に採用する。
3. 採用された各球種について、`pitch_type/<PitchType>` 配下の `normal_param` から `PitchSkill` のフィールドを生成する。

## チーム割り当て

`assign_team()` は `PlayerService::next_team(player.defense_skills.position)` を呼び出します。

| ステップ | 挙動 |
| --- | --- |
| 主探索 | `next_player_dist_team(position)` はその主ポジションの選手が最も少ないチームを返します。 |
| フォールバック | 探索が `AppError::NotFound` を返した場合、`next_random_team()` がランダムなチームを選択します。 |
| その他のエラー | コンテキスト付きで `AppError::Internal` としてラップされます。 |

## 永続化

生成されたプレイヤーは `SqlPlayerRepository::insert_player()` によって単一トランザクションで保存されます。

| テーブル | 保存されるデータ |
| --- | --- |
| `player_info` | チームID、名、姓、年齢、背番号。 |
| `batter_info` | `offense_skills.batter` が `Some` の場合のみ保存されます。打席側（右左）、打者タイプ、ゾーン適性、ホットゾーンスケール、ミート眼、スイング速度、スイングパワー、アタックアングル、バットコントロール、安定度を含みます。 |
| `running_skills` | 生成されたすべてのプレイヤーで保存されます。 |
| `defense_skills` | 主ポジション。 |
| `fielder_info` | 生成された守備スキルグループごとに1行。送球速度、走力、反応、準備時間、捕球、到達高、到達範囲を含みます。 |
| `pitcher_info` | `defense_skills.pitcher` が `Some` の場合のみ保存されます。投手レベルのスピン量と安定度を含みます。 |
| `pitch_skill` | 投手の生成された球種スキルごとに1行。 |
