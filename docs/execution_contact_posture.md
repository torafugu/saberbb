# 保存接触時刻からの姿勢・上下方向応答の診断

実行比率1/0.95/0.90/0.85を混ぜても、今回のStatcast比較では低EV・高VLA打球の不足が残った。接触時の姿勢と衝突応答を分けて調べるため、前回保存した接触時刻で状態を再構成する。新しい接触時刻の探索や、既定パラメータの変更は行わない。

## 実行

```sh
python3 tools/diagnostics/execution_contact_posture.py \
  --input-dir ../saberbb_execution_ratio_sweep \
  --source-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_execution_contact_posture
```

DBは既存の `SABERBB_DB` を利用する。明示する場合は `--db PATH` が優先。DBは前回の比率別診断を作ったものを使う。選手や試合を再生成しない。出力先は新しいフォルダを指定する。

`--input-dir` は比率別診断の出力先で、次の4ファイルが必要。

- `swing_execution_metadata.json`（version=2）
- `swing_execution_states.jsonl`
- `swing_execution_summary.csv`
- `swing_execution_ev_vla.csv`

`--source-dir` はその比率別診断の入力にした観測更新出力先。`observation_update_inputs.jsonl` と `observation_update_contacts.csv` を使う。上のフォルダ名は以前の実行例に合わせたものなので、実際の保存先が異なる場合だけ変更する。

既存statesにはボールの全状態・投球回転が保存されていないため、DBのcontact_stateとpitchingを読み取り専用で参照する。元の予測計画から開始姿勢を再生成し、保存されたplan/execution/deviationと照合する。各接触について保存時刻で一度だけ姿勢を評価し、従来と同じ3つの衝突応答を計算する。端部・曲線範囲外は採用範囲を変えずに保持する。

接触探索は0回。所要時間はJSONL/CSVの読取り・照合・ハッシュ計算と接触数に依存し、実ユーザーデータでは未測定。標準ライブラリのみで動作する。

## 再現検査

次の検査がすべて通った場合だけ、完了を示す `execution_contact_posture_metadata.json` を保存する。失敗した出力フォルダは途中結果であり、分析に使わない。修正後は別の新規出力先で再実行する。

- 元の観測CSV/JSONLの件数・選択行のキー整合、投球重複、保存statesの順序・キー・過不足
- DBの状態バージョン・入力妥当性、左右打席、予測誤差と基準時計
- 保存されたplan、実行パラメータ、基準ベクトル、開始時計、接触時のgain/角速度
- 保存接触時刻が元の探索窓内にあり、球とバットの間隔が接触許容範囲内であること
- 元の投球回転スカラーのSHA256
- scenario/scope/modelごとの試行数・時計不成立数・接触数・成功数・フェア方向数
- 成功例の平均EV/VLA、幾何接触例の平均法線z・傾斜・点速度
- 元の実行側EV/VLAヒストグラムの全セル件数・分母・割合
- インパルスからの速度再構成と、最終速度ベクトルからのEV/VLA再計算

保存接触時刻を再利用するため、接触が最初に発生した時刻であることを独立には証明しない。時計不成立は未評価。entry_timeがない行には空振り・開始時重複・探索未解決が含まれうるため、`no_saved_entry` として数え、すべて空振りへ置き換えない。

## 出力

| ファイル | 内容 |
| --- | --- |
| `execution_contact_posture_summary.csv` | 接触姿勢・上下方向速度・EV/VLAの層別集計 |
| `execution_contact_posture_selection.csv` | 法線角度帯ごとの幾何接触数、衝突応答の採否・方向。端部・曲線範囲外等も保持 |
| `execution_contact_posture_pairs.csv` | 同じ接触に対するnormal/finite応答の共通成功集合・共通フェア集合での比較 |
| `execution_contact_posture_checks.csv` | 再構成・再現一致の最大誤差 |
| `execution_contact_posture_coverage.csv` | 入力投球数、各状態の未評価・接触再構成・適格件数、`new_searches=0` |
| `execution_contact_posture_metadata.json` | 条件・元metadata・入力SHA256・回転由来・検証内容・限界 |

DB上の試合・能力値・衝突係数・乱数状態は変更しない。シナリオ・scope・cohort・stratum・モデルには重複があるため、合算しない。

## 読み方

最初に `model=finite_I0.00015_et0.4`、`scope=all_swings`、`cohort=fair_direction` を見る。`all_successful` も別に確認し、フェア方向への選択で偏りが強まっていないか比較する。

summaryの `stratum` は次の分割。各分割は同じ打球集合を別の切り口で集計する。

| stratum | band_1 | band_2 |
| --- | --- | --- |
| `overall` | all | all |
| `vla` | 最終VLAの帯 | all |
| `normal_relative_z` | 接触法線の仰角 | 入射球−バット点の相対上下速度 |
| `axis_normal` | 接触時のバット軸の仰角 | 接触法線の仰角 |
| `impact_axis` | 局所的な符号付きb/R | 接触時のバット軸の仰角 |

各指標は `n_*` と `avg_*` を持つ。未定義値を0で埋めない。`contacts` はその層の打球数、`low_ev` はEV<25m/s、`high_ev` はEV>=45m/s、`downward` はVLA<0度、`high_vla` はVLA>=50度の件数。

上下方向の速度は、各接触で次の恒等式を検査する。

`outgoing_z_ms = incoming_z_ms + normal_delta_z_ms + tangent_delta_z_ms`

- `normal_z` はバットから球へ向く法線の上下成分。正なら上向きの法線だが、入射速度もあるため上向き打球を保証しない。
- `normal_delta_z_ms` と `tangent_delta_z_ms` は各インパルスを球質量で割った上下速度変化。有限半径モデルの摩擦成分も含む。
- `normal_only_vla_deg` は、その応答の法線インパルスを固定したまま接線インパルスだけ除いたベクトルのVLA。法線と接線が連成するモデルなので、摩擦ゼロで解き直した別モデルの結果とは異なる。
- `tangential_vla_change_deg` は最終VLAから上のVLAを引いた値。平均ベクトルの角度ではなく、接触ごとの角度差の平均を記録する。
- `axis_elevation_deg` は保存された軸の向きに従う符号付き角度。`bat_point_attack_deg` は接触点速度の仰角。球の上下軸はworld zで、VLAは最終速度のzと水平速度から計算する。
- `local_signed_b_over_r` は瞬間の軸・相対速度が張る円柱断面での符号付き位置。回転軌道全体の最接近距離ではない。端部等で未定義ならnから外す。

上向きの法線を持つ幾何接触自体が少ないなら、上流の狙い・相対軌道・バット軸を調べる。上向き法線の接触が十分あるのに成功例が少ないならselectionで除外理由を確認する。成功してもVLAが低いなら、入射速度と法線/接線インパルスを確認し、pairsで同じ接触に対する応答差を見る。

pairsの `common_successful` は両モデルで成功した同じ接触、`common_fair` はさらに両方フェア方向のもの。`up_to_down` / `down_to_up` はnormalからfiniteへのVLA符号の変化。`avg_*_downward` / `avg_*_high_vla` は件数ではなく0〜1の割合。各モデルの全成功例を比べた結果と共通集合を混同しない。

これらは原因を切り分ける記述集計であり、バット角度や上下狙い誤差の修正値を決める校正ではない。Statcastとの期間・リーグ・バント等のcohort差、最終フェア判定の未実装も引き続き残る。

実行後は上の6ファイルを共有する。大きいstates JSONLや元のDBの共有は不要。

## 計画傾斜の小標本比較

姿勢と方向の関係が見えた後は、[計画傾斜の固定小標本比較](plan_tilt_sweep.md)を使い、同じ投球に異なる意図した傾斜の計画を適用できる。元の予測打点・時刻・理想速度を共通にし、支点高さや実際の点速度も監査する。軌道が変わるため新たな接触探索が必要だが、既定は結果と独立に選ぶ2,000投球に限定する。
