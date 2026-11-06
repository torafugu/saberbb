# 打撃戦略・選手の傾向・監督

2026-11-06時点。実装: [batting_personnel.rs](../../../src/domain/shared/batting_personnel.rs)、[batting_strategy.rs](../../../src/domain/strategy/batting_strategy.rs)、[人物リポジトリ](../../../src/repositories/batting_personnel_repository.rs)。

## データと判断の分離

| 層 | 所有・寿命・役割 |
| --- | --- |
| BatterCharacter | `Player.batting_character`。持続的な打撃傾向。能力値と別にDBへ保存 |
| BatterTendencies | 性格から導出する接触優先度・ライナー優先度・状況適応度 |
| SituationalBattingPreferences | 投球前のカウント・アウト・走者から毎回生成する重みと希望Intent |
| Manager | 独立したid・name・batting_policyを持つ人物 |
| ManagerInstruction | 監督が状況から生成する目的と理由。具体的な狙い位置は指定しない |
| BattingIntentDecision | 打者の希望と監督目的から選ぶIntent。元の希望も保持 |
| Planner | 選ばれたIntentと予測情報から狙いとスイング計画を生成 |

Intentや状況別PreferencesはPlayerの固定属性にしない。既存BatterType／PlateApproachによる振る・見送る判断とは別の層。
通常のGameServiceでも新経路を診断へ記録するが、ライブの接触・打球・スイング／見送りは既存経路のまま。

## 打者の性格と状況別選好

| JSON/CLI値 | 接触優先度 | ライナー優先度 | 適応度 |
| --- | ---: | ---: | ---: |
| contact_oriented | 0.8 | 0.2 | 0.4 |
| line_drive_oriented | 0.2 | 0.8 | 0.4 |
| adaptive | 0.5 | 0.5 | 1.0 |
| balanced | 0.5 | 0.5 | 0.5 |

接触スコアへ、2ストライクなら適応度×0.5、三塁走者あり・2アウト未満なら適応度×0.25を加える。
ライナースコアへ、2ボール以上0ストライク、または得点圏走者あり・2アウトなら適応度×0.25を加える。
合計で正規化して重みを求め、高い方を希望する。同点はContactFirst。
係数は凍結したv1の診断仮説であり、実データへの較正値ではない。

## 監督と調整

`manager` に人物、`team_manager` に現在の所属を保存する。1チームに監督1人、1監督に所属1チーム。未所属の監督も存在でき、移籍・交代後も人物を削除しない。`Team.manager` から試合状態と保存caseへスナップショットを渡す。

| 方針 | 指示 |
| --- | --- |
| let_batter_decide | 打者の希望Intentを採用 |
| put_ball_in_play | ContactFirstを指示 |
| seek_line_drive | LineDriveを指示 |
| situational | 下記の優先順で決定 |

situationalは、①2ストライク→PutBallInPlay、②三塁走者あり・2アウト未満→PutBallInPlay、③上記のライナー加点状況→SeekLineDrive、④その他→LetBatterDecide。
監督不在はNoManagerとして打者に任せる。指示がある場合は優先する決定規則で、確率的服従や反復交渉はない。

ContactFirstは予測中心を狙う。LineDriveは予測高さが0.6m未満なら中心、それ以上なら世界座標で15mm下を狙う。
実際の接触・EV・回転・結果からIntentを選び直さない。名前だけでフェア率や得点の改善を保証しない。

## 保存と互換性

`player_batting_character` は性格をJSONで保存する。旧JSON・未移行DB・新規生成選手の既定値はBalancedで、能力値から性格を推測せず追加乱数も消費しない。非既定値は明示的に設定する。
監督なしは架空の人物で埋めない。壊れた保存値や一部だけ存在する監督スキーマはエラーとする。

既存DBへの追加移行は次を使う。既存データを消す `--setup` と区別する。

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --locked --bin batting_personnel -- migrate
cargo run --release --locked --bin batting_personnel -- set-character --player-id 2 --character adaptive
cargo run --release --locked --bin batting_personnel -- set-manager --id 1 --name '監督A' --policy situational --team-id 1
```

IDは実DBの値に置き換える。現行Rust CLIはアプリ設定の `database_path` を読むため、これをSABERBB_DBと同じDBへ設定する。PythonのDB依存診断はSABERBB_DBを読む。DBパスのコマンドライン引数は使わない。
移行は既存データを保持し反復可能。set-managerのteam-id省略は人物の作成・更新のみ。同じIDへの別team-id指定で移籍する。TUI編集画面は未実装。

保存済みスイング状態の `batting_selection` に、性格・状況・重み・監督・指示理由・選択結果を記録する。意思決定は見送りを含む全投球で行うが、この証跡は保存済みスイング状態が対象。
[人物モデルの詳細](../../player_manager_batting.md)、[生成・監査手順](diagnostics.md)を参照。
