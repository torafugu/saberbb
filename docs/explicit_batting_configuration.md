# 監督目的・打者選好を設定ファイルで指定する

`intent_holdout run --batting-config PATH`で、チームごとの監督・コーチの目的と、選手ごとの打者選好を指定する。
対象は診断経路。ContactFirst／LineDriveの狙い・係数・選択の優先順位は固定し、ライブ打撃へ適用しない。
IDから割り当てる既存の`mixed-id-v1`は検査用に残す。

## 設定を作る

現在のアプリDBのチームID・選手IDを並べた編集用テンプレートを作成する。
DBは読み取り専用で開き、出力ファイルは新規作成する。既存ファイルは上書きしない。

```sh
cargo run --release --locked --bin intent_holdout -- config-template --output-file ../saberbb_batting_config.json
```

生成直後は全チームLetBatterDecide、全選手ContactFirst。目的のチーム・打者の設定を編集してから実行する。
名前や能力から選好を自動推定する処理はない。

最小の構成例は次のとおり。例のIDは実際のDBに存在するIDに置き換える。

```json
{
  "version": 1,
  "default_strategy": "let_batter_decide",
  "default_preferred_intent": "contact_first",
  "team_strategies": [
    {"team_id": 1, "strategy": "seek_line_drive"},
    {"team_id": 2, "strategy": "put_ball_in_play"}
  ],
  "player_preferences": [
    {"player_id": 101, "preferred_intent": "line_drive"}
  ]
}
```

| 項目 | 指定方法 |
| --- | --- |
| version | 1固定 |
| default_strategy | team_strategiesにないチームの目的 |
| default_preferred_intent | player_preferencesにない選手の選好 |
| team_strategies | team_idとstrategyの配列。同一IDの重複不可 |
| player_preferences | player_idとpreferred_intentの配列。同一IDの重複不可 |

strategyは`let_batter_decide`／`put_ball_in_play`／`seek_line_drive`、preferred_intentは`contact_first`／`line_drive`。
すべてのトップレベル項目が必須で、個別指定をしない配列は`[]`にする。
個別指定が既定値より優先する。テンプレートは全IDの個別指定を含むため、既定値だけを変えてもその行は変わらない。
既定値を使いたいIDの行は配列から除く。

監督指示と打者の選好は独立した設定。LetBatterDecideなら選好を使い、他の2目的なら対応する意図を選ぶ。
指示で選択意図を変えても、打者の元の選好は保存する。所属が変わっても選好は選手IDに結びつき、目的は現在のチームIDに結びつく。

## 実行と保存

設定を編集して確定したら、未使用seed・新しい出力フォルダで実行する。
以前の検証範囲27792までを除外する例。

```sh
cargo run --release --locked --bin intent_holdout -- run --seed 2026110601 --rounds 40 --exclude-through-game-id 27792 --batting-config ../saberbb_batting_config.json --prepare-schedule --output-dir ../saberbb_configured_seed_2026110601
```

`--batting-config`と明示的な`--batting-profile`は同時指定不可。設定ファイルを使うrunのprofileは`configured-v1`になる。
チームID・選手IDの重複、不正値、不明な列や列挙値、DBに存在しない個別指定IDは、日程追加・runフォルダ作成の前に拒否する。
選手IDの存在確認には`player_info`、チームには`team`を使う。DBに存在するが今回出場しないIDの設定は許可し、試合母集団を変えない。
監督指示はチーム単位、選好は選手単位で適用するため、ホーム／アウェイやロスターの並び順には依存しない。

runフォルダの`batting_config.json`に入力の正確なバイト列をコピーし、そのSHA256と固定ファイル名をmanifestの`batting_config`へ保存する。
各試合のcaseにも、保存ロスター全員へ既定値と個別指定を適用した具体的なbatting_settingsを保存する。
元の編集用ファイルを後から変更してもrunは変わらない。保存済みコピーを変更すると、次のcase準備・監査・replayでハッシュ不一致となる。
設定の凍結は生成開始後には変更できない。

生成は通常の試合結果をDBへ保存し、ラウンドを進める。日程準備と途中失敗の扱いは[seeded holdout](seeded_intent_holdout.md)と同じ。

## 監査・比較

```sh
python3 tools/diagnostics/seeded_holdout_audit.py --run-dir ../saberbb_configured_seed_2026110601 --output-dir ../saberbb_configured_seed_2026110601_audit
python3 tools/diagnostics/intent_selection_policy.py --input-dir ../saberbb_configured_seed_2026110601_audit --output-dir ../saberbb_configured_seed_2026110601_policy --replicates 2000 --bootstrap-seed 0
```

PythonのDBパスは引き続き環境変数`SABERBB_DB`から取得する。Rustのアプリ設定と同じDBを指定する。
方針比較スクリプトはDBを開かない。
監査は固定ファイル名・正確な設定ファイルのハッシュ・厳密なschemaを確認し、各caseのロスターと凍結設定からbatting_settingsを再構成して完全一致を検査する。
さらに[打者別監査](mixed_batter_intent.md)で実際の打者・攻撃側・指示・選好・選択・上書き理由を照合する。
別の設定を使ったcaseへの差し替えや、設定ファイルのパストラバーサル、後から結果用の列を追加することは許可しない。

出力と共有ファイルは従来どおり。まず次の4ファイルで設定の適用を確認できる。

- rust_collision_response_batters.csv
- rust_collision_response_batter_coverage.csv
- rust_collision_response_checks.csv
- rust_collision_response_metadata.json

方針比較の`recorded`は今回指定した設定による選択。均一の6通りとの同一投球比較も続けて使える。
打者別の生の率から設定の優劣や能力を推定せず、接触結果を選択入力へ逆流させない。
設定を結果に合わせて変更する場合は、新しい仮説として凍結し、未使用のholdoutで評価する。

過去のdefault／mixed-id-v1 manifestはそのまま監査できる。過去の試合を新しい設定のrunとして再ラベルしない。
replayには引き続き生成時と同じソース・コンパイラ・target・profileが必要。
