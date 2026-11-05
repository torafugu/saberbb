# 外部指定した打球意図をスイング計画へ接続する

検証済みの狙い方を固定したまま、呼び出し側が打球意図を明示し、予測情報から計画を生成する入口を追加した。
最初は次の2つの意図を選べる。

| 外部指定する意図 | 対応する固定方針 | 予測高 <0.6 m | 0.6≤予測高<0.9 m | 予測高≥0.9 m |
| --- | --- | --- | --- | --- |
| contact_first | contact_center | 0 mm | 0 mm | 0 mm |
| line_drive | line_drive_low_center | 0 mm | −15 mm | −15 mm |

意図の名前は達成する打球を保証しない。既存の`pre_swing_aim_seed2_frozen.json`から方針を読み、
狙い量・高さ境界・物理係数・速度構成比を調整しない。
カウント・走者・打者特性から意図を選ぶ処理と、フライ志向の定義は次の段階とする。

## この段階の接続範囲

現在のRust本体の実スイングと、診断用Pythonの予測ベースのスイング計画は別の経路にある。
今回は**Pythonの計画生成APIへ打球意図を接続する段階**で、Rustの打球結果へ新戦略を適用する変更は含まない。
これにより、狙い方の評価と本体の接触・衝突モデル移行を一度に変更することを避け、
意図から生成された計画が検証済み軌道と一致することを先に確かめられる。

## 計画API

`tools/diagnostics/batting_intent.py`を診断ツールのimportパスに置いて利用する。

```python
from batting_intent import BattedBallIntent, PlanningInput, frozen_planner
from swing_execution import SwingPlan

planner = frozen_planner()
inputs = PlanningInput(
    forecast_position_world_m=(0.1, 0.0, 0.8),
    forecast_time_sec=0.4,
    attack_angle_deg=10.0,
    bat_speed_ms=35.0,
    handedness_sign=1.0,
    execution_angle_error_deg=0.0,
)
planned = planner.plan(inputs, BattedBallIntent.LINE_DRIVE)
execution_plan = SwingPlan.from_saved(planned.forecast_plan)
```

上記は構文例で、予測・能力値は呼び出し元が供給する。既定の傾斜15度・yaw 0度・速度比1.0は検証時の設定。
必要なら`plan(..., tilt_deg=15., yaw_batter_deg=0., speed_ratio=1.)`で明示できる。
角度・速度の条件を変えた場合は、同じ性能が検証済みとは扱わない。

`PlanningInput`には、予測球中心・到達時刻・打者の速度とattack angle・左右・バット形状・既存の角度実行誤差だけを渡す。
接触法線・実際の到達位置・EV/VLA・接触成功は渡さない。狙い選択自体が参照するのは予測投球高と固定方針だけ。
`from_forecast_plan()`もこれらの入力だけを取り出し、保存されていた狙い点・接触結果・actual_pointは流用しない。
新しい時刻誤差・角度誤差や乱数抽選は追加せず、残余の独立遅延は既存診断と同じ0秒とする。

返される`IntendedSwing`は意図・対応方針・上下オフセット・既存形式の`forecast_plan`を持つ。
下側狙いは予定基準点・実行誤差適用後の基準点・支点へ反映する。予測球中心や到達時刻、同じ速度条件の軸・速度は保持する。
`forecast_plan`の`actual_*`は既存の角度実行誤差を適用したバット状態を表し、実球の最終状態から計画を補正した値ではない。
同じ計画形式を既存の`SwingPlan.from_saved()`へ渡すことで、速度パルスの開始姿勢まで生成できる。

## 実データでの接続確認

seed3の元キャッシュを使い、両意図を明示して計画を生成する。

```sh
python3 tools/diagnostics/batting_intent_probe.py \
  --input-dir ../saberbb_aim_policy_holdout_seed3 \
  --output-dir ../saberbb_batting_intent_seed3 \
  --intents contact_first line_drive
```

片方だけを実行したい場合は`--intents line_drive`などと指定する。
意図の抽選・混合比はなく、指定した各意図を同じ投球で個別に比較する。

入力はstates JSONL・metadata・sample・summary・EV/VLA・kinematicsを含む元の接触計算キャッシュ。
各投球・各傾斜／yaw／速度について、意図から新しく計画を生成し、選んだキャッシュの計画全フィールドと照合する。
評価可能な投球では、生成した計画から作り直した速度パルスの開始姿勢も照合する。
その後に対応する保存済みの接触・打球結果を集計する。狙いに必要な条件がない場合や計画が一致しない場合はエラーとする。

DB・接触探索・衝突再計算は不要。出力先は新しいフォルダを指定する。
この比較は**計画APIの接続と同等性の確認**であり、新しい独立標本での効果検証ではない。
同じseed3・同じ条件なら、前回の`contact_center`／`line_drive_low_center`と同じ点推定になる。

最初に次の4ファイルを共有する。

- `batting_intent_summary.csv`：意図別の接触・フェア方向・ライナー帯等の集計。
- `batting_intent_pairs.csv`：両意図の同一投球比較。単独指定ではヘッダーのみ。
- `batting_intent_checks.csv`：予測・計画・開始姿勢と元キャッシュの整合性。
- `batting_intent_metadata.json`：指定した意図、固定方針、入力ハッシュ、接続範囲。

追加で`batting_intent_selections.csv`に高さ別の狙い選択、`batting_intent_plans.jsonl`に
投球・意図ごとの生成計画を保存する。planログにはEV/VLAや接触結果を含めない。
時計不成立でも予測からの計画は保存し、評価対象の開始姿勢は空欄とする。空振りとは扱わない。

## 検証と次の段階

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_batting_intent.py'
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
```

境界値、左右打者、意図を変えた際の狙い・支点の変化と予測・速度の保持、事後情報を変えても不変な計画、
旧診断との集計一致、開始姿勢の照合、単独意図指定、DB・探索・衝突再計算の不使用を確認する。

この接続確認の後、打者戦略が状況に応じて`BattedBallIntent`を返す処理を追加できる。
その処理はスイング前に利用可能なカウント・走者・打者特性・予測等を入力とし、打球結果は評価側だけで扱う。
Rust本体への適用には、予測ベース計画・実行・接触検証の移植範囲を別途明示する必要がある。
