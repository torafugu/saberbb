# 診断・再現・holdout

2026-11-06時点。ここでは現在の入口と結果の読み方をまとめる。個別実験の係数・列・移行SQLはリンク先の仕様を参照する。

## 通常試合と診断の境界

| 経路 | 試合結果への適用 |
| --- | --- |
| Swing/Take、75%時間位置ずれ、符号付きHLA、10ms空中捕球探索 | 通常試合で使用 |
| Player/Manager由来Intent、予測更新、スイング計画・実行、3D接触、有限バット応答 | 診断記録。通常打球や得点へ戻さない |
| 速度・狙い・傾斜・yawのsweep、固定mixture、Statcast比較 | 保存データによる仮説比較。ライブ採用を意味しない |

診断JSONは `player_game_contact_state.state_json` に保存する。旧行に新しい証跡が存在するとは限らず、欠落を推測補完しない。
接触なし・適用対象外・未収束をEV=0や同じ意味の空振りへまとめない。coverageとchecksを先に確認する。

## 既存DB

既存DBは必要な `migrations/upgrades/` の追加移行を個別に適用し、新しい試合を生成する。`--setup` は表を再作成する初期化で、アップグレード用途では使わない。

| 対象 | 手順・診断SQL |
| --- | --- |
| 打撃接触・miss | [接触ログ](../../batting_contact_diagnostics.md)、[miss分類](../../contact_miss_diagnostics.md) |
| Swing/Takeと2ストライク | [判断診断](../../swing_decisions.md) |
| タイミングと方向 | [75%位置ずれ](../../timing_position_trial.md)、[符号付きHLA](../../contact_normal_hla.md) |
| 3D共通接触 | [共通接触座標](../../contact_state_coordinates.md) |
| 捕球・候補・空中probe | [捕球](../../fielding_catch_diagnostics.md)、[候補](../../fielding_candidate_diagnostics.md)、[10ms probe](../../fielding_air_probe.md)、[通常探索の比較](../../air_catch_search_validation.md) |
| Player/Manager | [打撃戦略の移行](batting_strategy.md) |

## 明示seedでの生成と監査

リポジトリのルートで実行する。Python診断のDBはSABERBB_DBで指定する。現行Rust CLIのアプリ設定 `database_path` も同じDBへ設定する。DBパス引数は使用しない。

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --locked --bin intent_holdout -- status --rounds 40
```

次の例のseed・既知データの除外境界・出力先は、生成前に自分の検証条件として決める。0は過去データを除外しないための例示値で、独立holdoutの証明にはならない。

```sh
cargo run --release --locked --bin intent_holdout -- run --seed 2026110603 --rounds 40 --exclude-through-game-id 0 --batting-profile player-context-v1 --prepare-schedule --output-dir ../people_holdout_new
python3 tools/diagnostics/seeded_holdout_audit.py --run-dir ../people_holdout_new --output-dir ../people_holdout_new_audit
python3 tools/diagnostics/intent_selection_policy.py --input-dir ../people_holdout_new_audit --output-dir ../people_holdout_new_policy --replicates 2000 --bootstrap-seed 0
```

runは通常の試合結果をDBへ保存しラウンドを進める。`--prepare-schedule` は不足する将来日程の作成とカーソル調整も行う。既定では日程不足を事前エラーにする。40は試合数ではなくラウンド数。
全体がDBとmanifestをまたぐ単一トランザクションではないため、失敗runは保持し、成功した試合だけを抜き出してholdoutとしない。新規出力先を使い、同時に日程・試合を変更しない。

`status: complete` は生成の完了であり、数値一致・coverage・戦略の有効性の合格判定ではない。
監査で保存case・明示seed・ビルド指紋・DB行の正確なハッシュ・Rust/Python再計算を照合する。同じビルドで単一caseをreplayする場合:

```sh
cargo run --release --locked --bin intent_holdout -- replay --run-dir ../people_holdout_new --game-id 123
```

123はmanifest内の実際のIDに置き換える。replayはDBへ書き込まない。[seed付きholdout](../../seeded_intent_holdout.md)に失敗状態・再現条件の詳細がある。

## 設定source

| 指定 | 用途 |
| --- | --- |
| default | 固定した既定の診断選好・指示 |
| mixed-id-v1 | 再現可能なID別の混合診断設定。人物性格の推定ではない |
| `--batting-config FILE` | 明示したチーム目的・選手選好をコピー・hashして凍結（configured-v1） |
| player-context-v1 | 保存したPlayer.batting_characterとTeam.managerから状況別に生成 |

batting-configと明示batting-profileは併用しない。手動設定を減らすには、次で既存IDに対する均衡ランダム設定を生成できる。assignment-seedは試合seedと独立している。

```sh
cargo run --release --locked --bin intent_holdout -- config-template --assignment-seed 2026110601 --output-file ../batting_config_new.json
```

これは診断用設定を作るだけで人物DBを変更しない。[明示設定](../../explicit_batting_configuration.md)、[ランダム生成](../../explicit_batting_configuration.md)、[人物経路](../../player_manager_batting.md)を参照。

## 結果の解釈と共有

人物経路の基本共有物は `rust_collision_response_batters.csv`、`rust_collision_response_batter_coverage.csv`、`rust_collision_response_checks.csv`、`rust_collision_response_metadata.json`。判断を追うときは `batter_decisions.csv` も確認する。
方針比較は `intent_selection_policy_` のsummary/coverage/checks/metadataの4ファイルを基本とする。

方針比較のper_pitchの分母は両候補が揃った診断投球であり、全試合の全投球ではない。fairは方向判定、ライナー角度帯は10°≤VLA<30°で、安打・得点ではない。
ゲーム単位bootstrapは同じ抽出で候補を比較するが、繰り返し登場する選手・チームの試合間依存は残る。個別打者の率を戦略の因果効果と解釈しない。
件数は完全一致、浮動小数点のゲーム合計は観測件数あたりの残差で検査する。結果を見て重みや境界を変えたら新仮説として凍結し、未使用seed・標本で再検証する。

詳細: [Rust衝突応答](../../rust_collision_response.md)、[ゲームbootstrap](../../rust_intent_game_bootstrap.md)、[選択方針比較](../../intent_selection_policy.md)。
