# Playerの傾向から監督との調整を経てIntentを作る

この経路は、Playerに属する持続的な性格と、その投球で生成する選好・意思決定を分ける。
通常の試合処理では新経路を診断ログへ記録する。接触・打球・スイング／見送りの本体処理は現行のまま。

| 層 | 所有者と役割 |
| --- | --- |
| BatterCharacter | Player.batting_character。能力値と別の持続的な打撃傾向。DBへ保存 |
| BatterTendencies | characterから生成する接触志向・ライナー志向・状況適応度 |
| SituationalBattingPreferences | カウント・アウト・走者の有無から生成する重みと希望Intent。毎投球で再計算 |
| Manager | 独立したID・名前・打撃方針を持つ人物。チームとの所属は別テーブル |
| ManagerInstruction | 監督が状況から生成する目的と理由。具体的な狙い位置は指定しない |
| BattingIntentDecision | 打者の選好を保持したまま監督目的と調整して選ぶIntent |
| Planner | 選ばれたIntentと予測情報から、既存の狙い・スイング計画を作る診断経路 |

Intentと状況別PreferencesはPlayerの固定属性にはしない。
以前のBattingPreferencesは明示的な診断設定用アダプターとして残す。新しい重み・状況フラグはSituationalBattingPreferencesに記録する。

## 持続的な打撃傾向

BatterCharacterは`contact_oriented`、`line_drive_oriented`、`adaptive`、`balanced`。
旧JSON・旧DBと新規選手の既定値はBalanced。生成器は能力から性格を推測せず、追加の乱数も消費しない。
非既定値は保存API／下記CLIで設定する。IDの偶奇・打撃結果からの割り当てはしない。
DBのplayer_batting_characterにJSONとして保存し、選手作成トランザクションでも保存する。

v1の傾向の係数は次の診断仮説。実データへの較正値ではない。

| character | 接触優先度 | ライナー優先度 | 状況適応度 |
| --- | ---: | ---: | ---: |
| contact_oriented | 0.8 | 0.2 | 0.4 |
| line_drive_oriented | 0.2 | 0.8 | 0.4 |
| adaptive | 0.5 | 0.5 | 1.0 |
| balanced | 0.5 | 0.5 | 0.5 |

投球前の状況だけを入力にする。2ストライクなら接触スコアに適応度×0.5、三塁走者あり・2アウト未満なら適応度×0.25を加算。
2ボール以上0ストライク、または得点圏走者あり・2アウトならライナースコアに適応度×0.25を加算する。
スコアを正規化して重みとし、高い側のIntentを希望する。同点はContactFirst。
係数は凍結したv1仮説であり、今回のholdoutを見て最適化した値ではない。
現段階では球種・予測位置・得点差などを追加の選択入力にしていない。予測高さは従来どおりPlannerの狙い位置の決定で使う。

## 独立した監督

ManagerはPlayerやTeamのenumではなく、`id`、`name`、`batting_policy`を持つ独立オブジェクト。
managerテーブルで人物を保存し、team_managerで現在の所属を管理する。監督は未所属でも存在でき、同じIDで別チームへ移動できる。
1チームに現在の監督1人・1監督に現在の所属1チーム。交代後も元の人物レコードを削除しない。
Team.managerへ読み込み、各試合のGameStateと保存caseは当該監督のスナップショットを保持する。

監督方針は`let_batter_decide`、`put_ball_in_play`、`seek_line_drive`、`situational`。
固定3方針は従来と同じ目的を指示する。situationalでは次の順に判定する。

1. 2ストライク：PutBallInPlay、理由two_strike_protection。
2. 三塁走者あり・2アウト未満：PutBallInPlay、理由runner_advancement。
3. 上記のライナー加点状況：SeekLineDrive、理由run_production。
4. その他：LetBatterDecide、理由batter_discretion。

打者のPreferencesを上書きせずに、監督目的がLetBatterDecideなら希望Intent、それ以外なら指示するIntentを選ぶ。
これは強い監督指示を優先するv1の調整規則。信頼度による交渉や確率的な服従モデルはまだ導入しない。
監督未配置なら架空のIDを作らず、NoManagerとして打者に任せる。

## DB移行と設定

Rust側のアプリ設定database_pathを使う。既存DBで最初に次を実行する。

```sh
cargo run --release --locked --bin batting_personnel -- migrate
```

この移行はCREATE TABLE IF NOT EXISTSだけをトランザクションで実行し、既存の試合・選手・監督データを消さない。繰り返しても設定をリセットしない。
通常のDB初期化（--setup）は使わない。
未移行DBは旧データ互換のためBalanced・監督なしとして読み込める。非既定の性格・監督を保存する前に上の移行を行う。
一部だけ監督テーブルが存在する破損スキーマや、壊れた保存値は中立値へ黙って置き換えずエラーにする。

例の選手・チームIDは実際のDBにある値を指定する。

```sh
cargo run --release --locked --bin batting_personnel -- set-character --player-id 2 --character adaptive
cargo run --release --locked --bin batting_personnel -- set-manager --id 1 --name '監督A' --policy situational --team-id 1
```

set-managerのteam-idを省略すると人物の作成・更新のみ。同じ監督IDに別team-idを指定すると所属を移す。
同じチームに別の監督を指定すると交代する。存在しないチームへの配置は人物更新ごとロールバックする。
Playerの性格も通常の選手読み込みで復元される。現段階ではTUIによる性格・監督の編集画面はない。

## 新経路を明示した診断run

既存のdefault・mixed-id-v1・configured-v1は固定診断設定として挙動を保つ。
保存したPlayerとManagerから生成する場合は、新しいprofileを使う。
このsourceでは固定設定のID別マップを使わず、Player.batting_characterとTeam.managerを参照する。
--batting-configによる固定選好との同時指定はしない。

```sh
cargo run --release --locked --bin intent_holdout -- run --seed 2026110602 --rounds 40 --exclude-through-game-id 28152 --batting-profile player-context-v1 --prepare-schedule --output-dir ../saberbb_people_seed_2026110602
python3 tools/diagnostics/seeded_holdout_audit.py --run-dir ../saberbb_people_seed_2026110602 --output-dir ../saberbb_people_seed_2026110602_audit
```

PythonのDBパスは引き続きSABERBB_DBから取得する。同じDBを指定する。
入力caseにPlayerの性格とManagerの人物・方針を保存する。DBで後から変更しても保存caseは変わらない。
通常のGameServiceも同じ生成経路を診断に使うが、ライブ接触・打球結果にはまだ適用しない。

## 判断証跡と監査

投球前にBatterCharacter→傾向→状況別Preferences、独立したManager→指示、両者の調整→Intentを生成する。
実際の打球・接触法線・結果は選択関数の引数に存在しない。
スイングの診断状態のbatting_selectionに、バージョン、打者ID、性格、傾向、状況、重み、監督、指示理由、最終決定を保存する。
見送りを含む全投球で意思決定は行うが、この診断ログ・監査の対象は保存済みスイング状態。見送りの記録を追加した変更ではない。

Python監査は保存caseのPlayer・Managerから独立実装で再計算し、全生成段階と従来の決定・候補selectedを照合する。
打者ID・所属・監督・性格・重みの不一致や生成証跡の欠落は拒否する。状況自体は投球前のGameStateから保存された入力であり、Pythonで試合履歴全体を再構成しているわけではない。
DB診断行の正確なハッシュ、case入力、全試合の明示seed・設定に引き続き結び付ける。必要なら同じビルドで完全な試合replayを行う。

batter_decisions.csvにはmanager_id、batting_character、manager_instruction_reason、contact_weight、line_drive_weightが追加される。
batters.csvも監督ID・性格を保持し、同じ打者でも状況で希望・指示が変われば別行に集計する。個別打者の生の率を戦略の因果効果とは解釈しない。
旧固定設定は人物由来の決定を捏造せず、これらの追加列を空欄にする。
既存のintent_selection_policy.pyも、その投球に記録された選好と監督目的からrecordedを集計できる。

共有する基本4ファイルは従来と同じ。

- rust_collision_response_batters.csv
- rust_collision_response_batter_coverage.csv
- rust_collision_response_checks.csv
- rust_collision_response_metadata.json

同じseedの固定設定との比較テストでは、診断ログ以外の試合結果全体が一致し、人物を含むcaseからのreplayも一致する。
判断モデルの係数、監督方針、通常打撃への接続を変更する際は、新しい仮説として扱い、未使用holdoutで検証する。
