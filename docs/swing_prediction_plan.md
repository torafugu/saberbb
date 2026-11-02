# 予測到達点・到達時刻から作るスイング計画の診断

## 目的と適用範囲

予測・スイング計画・実行誤差・実球との接触検証を分離する。
本体の各スイングで予測スナップショットを保存し、Pythonの診断で予測位置・時刻を使った軌道を比較する。
旧スイング判断、接触採否、EV/HLA/VLA、予測球の生成、乱数消費を変更する実装ではない。
既存の`expected_ball`を未校正の予測事前モデルとして再利用する。

## 保存とデータ生成

`player_game_contact_state.state_json`へ任意の`swing_prediction`を追加する。
保存状態本体はversion=1、sourceも従来どおり。追加オブジェクトは独自のversion=1、
source=`expected_pitch_prior_commit_observation_v1`を持つ。
既存テーブル・INSERT文・過去行は変更しないため、追加のDB移行は不要。
旧JSONはserde(default)で読み込み、新オブジェクトがない場合はNoneとする。
新しいRustコードで進めたスイングから保存する。見送りには保存しない。
予測に失敗した場合もinvalid_reasonを残す。

**既存の24112～24471には予測スナップショットがないため、今回の比較は新しい試合の生成が必要。**
過去の予測状態を実際の最終位置から推測して補完しない。
古い試合・既存の診断結果を保持し、新しく生成した試合ID範囲を指定する。
既存の状態テーブルがすでにある環境なら、main取り込み後にそのまま試合を追加生成できる。

```sh
python3 tools/diagnostics/swing_prediction_probe.py --db "/path/to/saberbb.db" --first-game-id 25000 --last-game-id 25359 --output-dir ../saberbb_swing_plan
```

25000～25359は構文例。実際に今回のコードで生成したIDへ置換する。
出力先には未作成のディレクトリを指定する。

## 情報の境界

| 層 | 受け取るもの | 出力 |
|---|---|---|
| 観測収集 | シミュレーションの実軌道、既存の予測事前モデル | 確定時点の球位置と推定速度・加速度 |
| 到達予測 | 観測時刻・位置、推定速度・加速度だけ | 予測到達位置・時刻 |
| スイング計画 | 予測到達位置・時刻、打者の設定、実行角度誤差 | 支点、予定軸、速度、実行後の軌道 |
| 接触検証 | 固定された計画と実球の軌道 | 接触・非接触、打球速度 |
| 評価 | 計画と実球の最終状態 | 予測誤差、比較条件の差 |

Rustの`forecast_plate_crossing()`とPythonの`forecast()`は、実際の最終位置・到達時刻・接触結果を引数に持たない。
Pythonの`plan_swing()`も実軌道を受け取らない。
実球の情報を持つ観測収集と接触検証を、予測・計画APIから分ける。

観測収集は、現在の一定加速度・終点整合の実軌道から**確定時点の正確な位置**を取り出す。
その実軌道の生成に最終状態を使うことと、予測器へ最終位置を渡すことを区別する。
現段階の観測器は誤差のないシミュレーション上のセンサーであり、人間の知覚を検証済みのモデルではない。

## 観測と予測の定義

確定時刻は`0.6 * expected_ball.flight_time`とする。
実球の将来の到達時間から確定時刻を決めない。
確定が実際の本塁面到達以後になる場合は、収集側が`commit_at_or_after_actual_plate`として無効にする。
この判定は観測が過去にあることの確認で、予測器への真の到達時刻の入力ではない。

実球・予測事前モデルの速度は既存`velocity_at_plate()`と同じ一定加速度近似から求める。

```text
actual_v0 = actual_plate_velocity - actual_acceleration * actual_flight_time
observed_position = actual_release_position + actual_v0 * commit
                    + 0.5 * actual_acceleration * commit²

estimated_velocity = prior_plate_velocity + prior_acceleration * (commit - prior_flight_time)
estimated_acceleration = prior_acceleration
```

推定速度・加速度は、従来から生成している別の`expected_ball`を事前モデルとして使う。
その予測球の到達コースは事前モデルの速度推定に含まれるが、実際の最終コースは含めない。
この予測球は現在`create_pitch()`で独立に生成されており、打者の信念分布として校正されたものではない。

現行モデルのY加速度は0なので、Y=0までの時間を次の式で解く。

```text
remaining = -observed_position.y / estimated_velocity.y
forecast_time = observation_time + remaining
forecast_position = observed_position + estimated_velocity*remaining
                    + 0.5*estimated_acceleration*remaining²
```

Y>0、推定速度Y<0、Y加速度0、有限値を検証する。
実球の速度をそのまま推定速度として流用しない。
保存したforecastと、観測・推定ベクトルからPythonで再計算した値を独立に照合する。

## 予定軌道と実行誤差

狙い打点は予測到達位置。予定バット角は、その予測高さを使った既存の10～60°の規則で求める。
実行角度誤差も、同じ予測コースで既存の決定論的関数を評価する。
この関数の抽出は本体側の同じ計算を共有するためで、既存スイングの計算順序上の乱数呼び出しは変えない。
本体のスイングは引き続き従来の入力を使う。

予定attack angleは打者の設定値、予定速度は打者のswing_speedを使う。
旧actual attack angleに含まれるタイプ補正・乱数補正は計画へ流用しない。
実行側は共通支点から軸・打点位置・支点からの相対速度を同じ角度誤差で回転する。

```text
ideal_point = forecast_position
ideal_axis = axis_from(predicted_height, batting_side)
ideal_velocity = (0, B*cos(configured_attack_angle), B*sin(configured_attack_angle))
pivot = ideal_point - L*ideal_axis
pivot_velocity = ideal_axis*dot(ideal_velocity,ideal_axis)
omega_ideal = cross(ideal_axis,ideal_velocity-pivot_velocity)/L

actual_axis = R*ideal_axis
omega_actual = R*omega_ideal
actual_point = pivot + L*actual_axis
actual_velocity = pivot_velocity + cross(omega_actual,L*actual_axis)
```

旧adapted X/Zは加算しない。予測位置のずれが空間誤差をすでに表しているため。
独立した新しい打点ノイズや角度誤差倍率も追加しない。
bat_controlは予測コースにおける既存の角度誤差規則へ反映する。
batting_eyeは記録するが、今回の新しい事前モデルやセンサーの誤差分布にはまだ反映していない。

## 時刻誤差の二重計上を避ける

旧timing_offsetは実球と予測球の到達時間差を含むため、新しい予測時刻へ足さない。
新計画は予測到達時刻で位相0となるようにし、独立な実行遅延は今回0と明示する。

```text
nominal_swing_time = forecast_time + residual_execution_delay
residual_execution_delay = 0
```

既存の探索は実球の本塁面時刻を座標原点とするため、そこで使うdelayは
`forecast_time - actual_plate_time + residual_execution_delay`。
絶対時刻Tで位相を展開すると、

```text
s = (T - actual_plate_time) - delay
  = T - forecast_time - residual_execution_delay
```

となり、実球の到達時間は座標変換で相殺される。
actual_plate_timeをスイング予定時刻の決定へ使う処理ではない。
0タイミング条件は各診断条件内の参考比較。旧タイミング誤差の追加や再抽選はしない。

## 同じ球で比較する3条件

| planning_condition | 定義 |
|---|---|
| actual_anchor_control | 前段の共通支点v2。実コースを使う旧保存状態を再利用する比較用の対照 |
| forecast_oracle_time | 新計画の位置・姿勢・速度を使い、時刻だけ実際の本塁面時刻に固定する対照 |
| forecast_position_time | 新計画の位置・姿勢・速度と予測時刻を使う候補 |

oracle_timeは位置側の影響と時刻側の影響を分けるための評価専用条件。
プレイヤーが知り得る情報だけで動く候補として扱うのはforecast_position_time。
actual_anchorとの比較では、狙い位置だけでなく基準角・attack angle・実行誤差の入力も変わる。
時刻だけの影響はoracle_time→forecast_position_timeの同一球比較で確認する。

L=0.6/0.9/1.2m、接触区間±0.16m、有効半径0.07m、局所探索±60ms、e=0.5、M=0.6kgが既定。
探索の中心時刻は実球の物理的検証用の時計で、計画関数へ渡さない。
未収束・開始時近接・非接近は非接触とは別に出力する。
これらの仮定を現実のContact率に合わせて校正したわけではない。

## 出力と確認順

| ファイル | 内容 |
|---|---|
| swing_plan_coverage.csv | 全スイングキー・有効な接触状態・有効予測・試行件数 |
| swing_plan_status.csv | 予測なし・収集失敗・不正な保存値・有効予測の件数 |
| swing_plan_checks.csv | 予測位置・時刻・予定角・実行角度誤差・確定時刻の再構成残差 |
| swing_plan_forecasts.csv | 1球1行の予測X/Z/時刻誤差と打者能力。真の値は評価専用 |
| swing_plan_summary.csv | 各条件の接触数・EV/VLA・法線速度比・EV40以上率 |
| swing_plan_transitions.csv | 2つの対照比較の状態遷移 |
| swing_plan_pairs.csv | 両条件とも力積正の同一球でのEV/HLA/VLA比較 |
| swing_plan_inputs.jsonl | 予測入力だけから作った支点・軸・速度・予定時刻 |
| swing_plan_contacts.csv | 全有効予測×半径×条件の接触明細 |
| swing_plan_metadata.json | 事前モデル・観測器・形状・係数・探索の仮定 |
| contact_3d_status.csv / contact_3d_quality.csv | 共通の接触状態・品質集計 |

旧状態だけの範囲を指定するとmissing_predictionとなり、結果を捏造しない。
forecast誤差は予測−実際、時刻も正なら予測到達が遅い。
summaryの平均は各条件で接触した部分集合、pairsは両条件とも力積正となった集合。
タイミング帯は各条件に適用したdelayに基づくので、同じ帯名でも条件間で球集合が異なりうる。
全帯域の比較はtiming_band=all、同一球比較はpairsを使う。
all行と帯域別行を合算しない。旧分類は比較ラベルで、計画・衝突計算へ渡さない。

## 検証と限界

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
python3 -m unittest discover -s tests/sql -p 'test_*.py'
cargo test swing_prediction
cargo test optional_prediction
```

Rust・Pythonに同じ解析例の到達位置・時刻テストを追加。
Pythonでは予測・計画に実際の終点を渡さない境界、実球変更が接触検証だけに効くこと、
タイミングの一回適用、左右鏡映、保存予測の再構成、古い状態の欠損扱い、DB不変性・対照条件の分母を確認する。
Rustでは旧JSON互換と追加オブジェクトの往復も確認するテストを追加。
作業環境にRustツールチェーンがないため、Rustテストは未実行。

この段階は情報の境界を明示した診断候補。既存expected_ballの生成を、打者の認識・履歴・
batting_eyeに基づく予測分布へ置き換える作業や、残余の独立実行誤差の校正は今後の課題。
本体適用や球速・反発係数の調整に先立ち、新しい試合で予測誤差分布と接触結果を確認する。

## 観測更新の追加診断

3時点の履歴と観測誤差の感度比較は[観測更新診断](observation_updated_swing_plan.md)を参照。
この追加診断を使う場合は履歴を保存するコードで新規試合を生成する。
