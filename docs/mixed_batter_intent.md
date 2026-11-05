# 監督指示と打者選好を混在させる診断

均一設定の6通りが仕様どおり動くことを確認した次の段階として、試合内で打者選好を混在させる。
監督・コーチの目的と打者の選好を別々に保持し、選択意図・理由を打者単位で追跡する。
新しい打撃能力モデルや現実の選好推定ではなく、二層の選択処理を検査するための人工的な固定設定。

## 事前に固定するmixed-id-v1

| 入力ID | 割り当て |
| --- | --- |
| team_id % 3 = 0 | LetBatterDecide |
| team_id % 3 = 1 | PutBallInPlay |
| team_id % 3 = 2 | SeekLineDrive |
| 正のplayer_idが偶数 | ContactFirstを好む |
| 正のplayer_idが奇数 | LineDriveを好む |

能力値、打順、予測や接触の結果、乱数は割り当てに使わない。ロスターの並び順やホーム／アウェイでも変わらない。
永続化された正の選手IDを使い、同一試合の重複IDは拒否する。
監督が打者に任せる場合は選好を使い、接触優先／ライナー優先の場合は対応する意図を選ぶ。元の選好は上書きしない。
ContactFirstは予測中心、LineDriveは予測高さ0.6m未満なら中心、それ以上なら15mm下を狙う凍結規則を維持する。

`--batting-profile mixed-id-v1`は生成開始前にmanifestへ保存され、全ロスターから作る具体的な設定も各caseに保存する。
省略時は従来の`default`（両チームLetBatterDecide・全打者ContactFirst）。過去manifestにprofileがない場合もdefaultとして照合する。
監査側はprofileから設定を独立に再構成し、caseの設定との完全一致を確認する。任意設定を無条件で許す変更ではない。

## 実行

以前の40ラウンドのholdoutは保持し、未使用seedと新しい出力フォルダで新規に生成する。
以下は既知の評価範囲の末尾27432までを除外する例。

```sh
cargo run --release --locked --bin intent_holdout -- run --seed 2026110503 --rounds 40 --exclude-through-game-id 27432 --batting-profile mixed-id-v1 --prepare-schedule --output-dir ../saberbb_mixed_seed_2026110503
python3 tools/diagnostics/seeded_holdout_audit.py --run-dir ../saberbb_mixed_seed_2026110503 --output-dir ../saberbb_mixed_seed_2026110503_audit
```

Rustは従来のアプリ設定`database_path`、Python監査は環境変数`SABERBB_DB`を使う。同じDBを指定する。
生成は通常の試合結果をDBへ保存してラウンドを進める。監査は読み取り専用で、新しい出力フォルダだけを作る。
`--prepare-schedule`の日程作成・シーズン移行と失敗runの保持については[seeded holdout](seeded_intent_holdout.md)を参照。

必要なら、同じ監査キャッシュで均一6通りと記録済み混在選択を比較できる。

```sh
python3 tools/diagnostics/intent_selection_policy.py --input-dir ../saberbb_mixed_seed_2026110503_audit --output-dir ../saberbb_mixed_seed_2026110503_policy --replicates 2000 --bootstrap-seed 0
```

ここでの`recorded`が今回の混在選択。均一設定の単純平均で作るのではなく、各投球に記録された監督指示とその打者の選好から集計する。

## 打者IDと選択の監査

新しい診断状態には`intent_batter`として実際の打者IDと攻撃側（is_top）を保存する。
監査は保存caseのホーム／アウェイのロスターから所属チームを特定し、その打者に適用される指示・選好・期待する選択理由を計算する。
観測診断に記録されたgame_id、count_seq、決定内容と照合し、選択候補のselectedとも一致させる。
接触法線・打球結果を選択入力にはしない。混在profileで有効な状態に打者IDがない場合や、所属・決定が不一致の場合は出力作成前に拒否する。

元の監査7ファイルに加え、次の3ファイルを出力する。

| ファイル | 内容 |
| --- | --- |
| rust_collision_response_batter_decisions.csv | 1診断状態ごとのgame_id・count_seq・チーム・打者・攻撃側・指示・選好・期待意図・記録意図・理由・照合状態 |
| rust_collision_response_batters.csv | チーム／打者／指示／選好／意図／理由ごとの状態数・確認済み決定数・上書き数・比較可能な投球数・選択した候補の接触／フェア／角度帯集計 |
| rust_collision_response_batter_coverage.csv | 打者情報欠落、確認済み決定、観測診断欠落、比較可能投球、監督×選好の各組み合わせの確認件数 |

`manager_override`は指示があるだけではなく、確認済みの選択意図が元の選好と異なる件数。
`batters.csv`の集約キーは事前設定から期待する意図・理由で、実際に照合できた数は`verified_decisions`に分ける。
観測診断を生成できなかった状態は`missing_observed_decision`として残し、投球ごとの実際の選択意図・理由を捏造しない。
接触結果を比較できない状態も`states`から消さず、結果の率は`comparable_pitches`を分母にする。分母0なら率は空欄。
比較可能な投球の中では、探索不能・応答対象外も分母に保持する。

打者追跡は有効な保存済みスイング状態が対象で、見送りを含む全投球・全打席ではない。
外側の無効状態や候補計画の欠落は従来の監査coverageでも確認する。
古いdefaultログの打者情報は推測で復元せず、`missing_batter_context`として数える。
新しいcaseは具体的な設定を含むので、生成時と同じビルドによる従来のreplayで混在設定も再現できる。

共有する基本ファイルは次の4つ。

- rust_collision_response_batters.csv
- rust_collision_response_batter_coverage.csv
- rust_collision_response_checks.csv
- rust_collision_response_metadata.json

metadataには生成manifestと打者監査coverageを含める。個別投球の追跡が必要な場合だけ`batter_decisions.csv`も使う。

## 解釈

同じseed・入力で混在設定とdefaultを比較するテストでは、診断接触ログを除く試合結果全体が一致する。
ライブ打撃・ゲーム乱数・物理係数は変えず、選択規則の配線と記録を検査する段階。
打者別の率には投手・チーム・遭遇投球の違いが含まれ、個別能力や監督指示の因果効果とは扱わない。
ID割り当てを実在の性格モデルと解釈せず、今回の結果を見て有利なID群を選び直さない。
実際の監督目標、能力や性格からの選好生成、カウント別の判断、ライブ適用はこの確認を踏まえて別途設計する。
