# バット軸の生成元と支点の監査

`tools/diagnostics/bat_axis_origin_audit.py` は、保存済みの観測更新試行から、予測高さ → 軸傾き → 支点 → 接触時の軸の関係を集計する。衝突探索、有限質量応答、接地後軌道、投球回転の再計算は実行しない。DBは不要で、入力を変更しない。

```bash
python3 tools/diagnostics/bat_axis_origin_audit.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_axis_origin_audit \
  --variants observed_sigma_0.005_r0.9
```

`--input-dir` は有限半径診断の出力先ではなく、次の4ファイルがある**観測更新診断の出力先**を指定する。

- `observation_update_inputs.jsonl`
- `observation_update_contacts.csv`
- `observation_update_metadata.json`
- `observation_update_coverage.csv`

既存の出力先への上書きは拒否する。`--variants` を省略すると全条件を集計する。指定した場合も全入力の姿勢再構成・対応キー・件数・重複・条件の完全性を検査する。全ファイルのハッシュ計算とJSON/CSV読み込みは必要なので、実データでの処理時間は未測定。

## ソースを追って確認したこと

1. `src/domain/resolver/batting_resolver.rs: calculate_bat_angle` は高さから `clamp(30 - 15 * normalized_height, 10, 60)` 度を生成する。`calculate_dynamic_attack_angle` はゲーム本体で別の速度角を計算し、軸傾きとの差に0.35を掛けて結合する。
2. `src/domain/shared/contact_state.rs: snapshot` は軸を `(h*cos(theta), 0, sin(theta))`、速度を `(0, speed*cos(attack), speed*sin(attack))` として別々に生成する。hは右打者+1、左打者-1。これは観測された姿勢ではなく診断用の代理状態。
3. `src/domain/shared/swing_prediction.rs` は予測コースから軸角を決め、速度角には打者の `attack_angle` を保存する。Pythonの `swing_prediction_probe.plan_swing` も両者を別々に使用する。ゲーム本体の動的速度角結合を、この予測診断に自動的には適用していない。
4. `observation_update_probe` は観測更新した予測高さから軸角を再計算するが、実行角誤差と速度角は保存されたprior条件から固定している。実際の到達位置から計画姿勢を決める処理ではない。
5. `plan_swing` は `pivot = forecast_target - lever * ideal_axis` を用いる。正のthetaでは支点が予測打点より低くなる。速度の軸方向成分を支点の並進として残し、残りを回転で再現する。実行角誤差は左右に応じたY軸回転で、軸・角速度を一緒に回し、共通支点を保つ。
6. `FrameMotion.pose` は `phase = time_offset - delay` で軸を回す。接触時の傾きは計画角・実行誤差・接触までの回転を含む。接触時のunsigned tiltをそのまま計画角とみなすことはできない。

特に5は検証すべきモデル仮定。予測打点z=0.50m、theta=45度、lever=0.9mなら支点zは約-0.1364mになる。この値は式から確定するが、実データでの頻度は集計結果を待つ。支点は実測した手元や身体関節と同一ではなく、地面より低い代理支点を数えただけで衝突応答の誤りとは判定しない。符号反転も、円筒軸の向きだけの変更と支点を含む軌道の変更では意味が異なるため、自動修正しない。

## 出力と読み方

| ファイル | 内容 |
| --- | --- |
| `bat_axis_origin_summary.csv` | 全体・推定左右・計画傾き・支点高さ・計画/接触傾きのクロス集計 |
| `bat_axis_origin_checks.csv` | 軸、支点、速度、角速度、実行誤差回転、接触時位相の再構成誤差 |
| `bat_axis_origin_coverage.csv` | 全入力行数と集計対象行数 |
| `bat_axis_origin_examples.csv` | 支点z<0の先頭最大30件。代表サンプルではない |
| `bat_axis_origin_metadata.json` | 入力ハッシュ、対象条件、解釈上の制約 |

- `overall` は全試行が分母。`contact` は幾何接触で、有限半径モデルの採用可否やフェア判定を条件にしない。接触しない試行も計画姿勢の平均に含む。
- `planned_entry_tilt` は `status=contact` の行だけ。重複するstratumやscopeは合算しない。
- `proxy_pivot_z_m` は計画基準時刻の支点高さ。`entry_proxy_pivot_z_m` は並進を含む接触時刻の支点高さ。`proxy_pivot_below_ground_rows` は前者が0未満の行数。
- 各指標に `n_` を付け、接触時の値がない行を0として平均しない。全試行の計画値と接触例だけの計画値は別の分母。
- `barrel_minus_pivot_z_m` は実行誤差を加えた軸の基準点と支点の高さ差。`entry_barrel_minus_pivot_z_m` は接触位相での基準点の高さ差で、実際の接触位置の軸方向ずれを足した値ではない。
- `phase_rotation_deg` は角速度×経過位相の符号付き回転量。軸傾きそのものではない。
- 左右は保存された理想軸のX符号から推定する。元DBの打席情報との独立照合ではない。鏡映の単体テストでは、支点Xの反転・Zの維持、同じ傾き/速度角を確認する。

この監査は保存姿勢の構成・時刻・接触軸と速度の一貫性を検証する。保存された観測履歴から予測位置を再推定したり、接触探索の正しさ、法線応答、実測姿勢との一致を検証したりはしない。元のJSONLとCSVが生成順に対応することが必要で、片方だけを並べ替えた入力は拒否する。

次に比較するべきものは、計画傾きと接触傾きの差、支点高さの分布、低い支点への接触の偏り。手元/身体から姿勢を構成するモデルへ進めるかは、この分布を見て判断する。
