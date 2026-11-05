# Rustの打球意図・予測計画を診断記録へ接続する

Pythonで検証した`contact_first`／`line_drive`をRustへ移植し、まず本体の診断記録へ保存する。
同じ予測入力ならPythonと同じ基準姿勢・速度・スイング開始姿勢を生成することを確認する段階である。
本体で使う打球意図を選ぶ処理や、生成計画を接触・衝突判定へ適用する変更は含まない。

## RustのAPI

`domain::strategy::batting_strategy::BattedBallIntent`に2つの意図を追加した。

| 意図 | JSON表現 | 固定した狙い |
| --- | --- | --- |
| ContactFirst | contact_first | 予測球中心 |
| LineDrive | line_drive | 予測world zが0.6 m未満なら中心、0.6 m以上なら−15 mm |

`domain::shared::forecast_swing_plan`の`PlanningInput`は予測球中心・到達時刻、打者のattack angle・速度・左右、
既存の角度実行誤差、バット形状だけを持つ。実球の到達位置・時刻、接触法線、EV/VLA、旧接触結果は引数に持たない。
`plan_intended_swing(&input, intent, settings)`が予定姿勢を、`execution_plan(&plan.forecast_plan, settings)`が
sin²速度パルスに対応した開始姿勢を生成する。

既定設定は傾斜15度・yaw 0度・速度比1.0、lever 0.9 m・半長0.16 m、duration 0.18秒・anchor 0.5。
予定努力は1.0、残余の独立遅延は0秒。速度・角度誤差の追加抽選や物理係数の変更はない。
意図の名前は結果を保証しない。高さは選手別ストライクゾーンで正規化していない。

## 本体での診断記録

既存の`attach_prediction()`が有効な`SwingPredictionInput`を記録するとき、
両意図の計画を追加で計算し、`player_game_contact_state.state_json`の任意フィールド`intent_plans`に保存する。

| 記録 | 内容 |
| --- | --- |
| version / source | 1 / forecast_intent_shadow_v1 |
| input | 計画に使った予測と打者設定の限定入力 |
| settings | 固定した傾斜・yaw・速度比・パルス設定 |
| available_time_sec | 既存の観測確定時刻 |
| plans | 2意図それぞれの予定姿勢、開始姿勢または未評価理由 |

開始がリリース前なら`execution_starts_before_release`、観測確定前なら`execution_starts_before_plan`。
この場合も予測から生成した予定姿勢は残し、評価対象の開始姿勢をNoneとする。
収集された予測自体が無効の場合は従来の`swing_prediction.invalid_reason`を残し、意図計画を生成しない。

新フィールドは`serde(default)`と`skip_serializing_if`で任意とし、旧JSON・テーブル・INSERT文はそのまま使う。
DB移行は不要。見送り・打球意図の抽選・ゲーム乱数・実接触・打球結果へは計画を戻さない。
両意図は診断用の比較候補で、本体がどちらかを選んだことを意味しない。

**予測器の違いにも注意する。** この記録が使うのはRustの既存expected-ball priorに基づく保存予測である。
seed3で利用したPythonの`observed_sigma_0.005_r0.9`観測更新モデルは今回移植していない。
この段階で検証するのは「同じ予測なら同じ計画を出すか」であり、seed3と同じ接触性能を本体で得たという意味ではない。

## Pythonとの照合

main取り込み後のRustで新しい試合を生成し、診断記録をDBへ保存する。
既存のseed3キャッシュや古い試合のJSONにはRustの新フィールドがないため、後から結果を見て補完しない。
次のゲームIDは構文例であり、**今回生成した実際の範囲に置き換える**。

```sh
python3 tools/diagnostics/rust_intent_plan_audit.py \
  --db /path/to/saberbb.db \
  --first-game-id 26000 --last-game-id 26009 \
  --output-dir ../saberbb_rust_intent_plan_audit
```

`SABERBB_DB`を設定していれば`--db`を省略できる。読み取り専用トランザクションを使用し、DBを変更しない。
接触探索・衝突再計算・Statcastデータは不要。出力先は新しいフォルダを指定する。

監査では、既存の`swing_prediction.input`から限定入力を取り出し、新しい`intent_plans.input`と一致するか確認する。
その後Pythonの`IntentPlanner`と`SwingPlan`で両意図を再生成し、Rustの予定姿勢・開始姿勢と未評価理由を照合する。
数値残差が1e-8を超える場合、方針・意図・時刻状態が食い違う場合はエラーにする。

最初に次の3ファイルを共有する。

- `rust_intent_plan_coverage.csv`
- `rust_intent_plan_checks.csv`
- `rust_intent_plan_metadata.json`

詳細キーと意図ごとの状態は`rust_intent_plan_details.csv`に保存する。通常は共有不要。
coverageの`matched_states`／`matched_plans`で予定姿勢を照合した件数を、`matched_execution_plans`で開始姿勢を照合した件数を確認する。
時計不成立は`matched_clock_invalid_plans`として別に保持する。空振り数ではない。
旧記録は`missing_intent_plan_states`、予測無効は`prediction_invalid_states`などに数える。
照合対象がゼロならmetadataは`no_comparable_shadow_plans`となり、一致確認済みとは扱わない。

## 共通テストデータと検証

`tests/fixtures/intent_plan_contract.json`はPythonの計画APIで生成した23ケースで、
両意図、0.6 m／0.9 m境界、左右打者、角度誤差、速度比、yaw、異なるanchor、速度ゼロ、時計不成立を含む。
Rustはこのファイルを読み、計画と開始姿勢を1e-9以内で照合する。
Pythonのテストも現行APIから再生成してファイルと一致することを確認する。
Rustの方針オフセットは元の固定JSONとも照合する。

```sh
cargo test --lib forecast_swing_plan
cargo test --lib swing_prediction
cargo test --lib contact_state
python3 -m unittest discover -s tests/diagnostics -p 'test_rust_intent_plan_audit.py'
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
```

共通データの再生成が必要な場合は、変更を明示的にレビューする。

```sh
python3 tools/diagnostics/intent_plan_contract.py \
  --output tests/fixtures/intent_plan_contract.json
```

同じ予測からの計画一致を確認した後、観測更新の移植範囲と本体の実行・接触検証への接続を検討する。

観測更新後の予測と、監督・コーチ／打者の判断を分けた記録は
[観測更新と打者戦略の階層](observed_batting_strategy.md)を参照。
