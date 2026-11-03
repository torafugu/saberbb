# 計画傾斜と水平向き（yaw）の分離診断

傾斜0度では高角度打球が増える一方、HLAが中央に集中し、15度では高角度打球が打者基準HLAの負側へ揃う傾向があった。基準バット軸の水平向きを独立に変え、上下・左右の分布を同時に比較する。

## 実行

```sh
python3 tools/diagnostics/plan_tilt_yaw_sweep.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_plan_tilt_yaw_sample \
  --sample-size 1000 \
  --sample-seed 0 \
  --tilts-deg 0 15 \
  --yaws-deg -10 0 10
```

DBは従来と同じ `SABERBB_DB` から読み取り専用で取得する。明示的な `--db PATH` が優先。入力は元の観測更新出力。選手や試合を再生成せず、出力先は新しいフォルダを指定する。

既定は1,000投球に対し、傾斜2×yaw3の6条件と保存計画の対照1条件を実行する。接触探索は最大7,000回で、時計不成立は全条件で未評価とし探索しない。対照は投球ごとに一度だけ探索する。実ユーザーデータでの所要時間は未測定。

標本選択は以前と同じ `plan-tilt-v1` のキー・seedによるハッシュ順。同じ元データ・seedなら、今回の1,000投球は前回2,000投球の標本に含まれる。全条件を同じ集合で比べるため、yaw=0と保存傾斜対照もその1,000投球で再計算する。角度や発生確率の自動校正は行わない。±10度は明示的な感度仮説で、実測された計画分布ではない。

## yawの定義

傾斜をt、打席符号をh（右=+1、左=-1）、打者基準yawをyとすると、理想バット軸は

`axis = (h*cos(t)*cos(y), cos(t)*sin(y), sin(t))`

world z軸の周りに `h*y` 回転する定義。左右打席で幾何学的に鏡映する。yawはバット軸の向きであり、打球HLAの指定値ではない。正負を引っ張り・流し打ちの指示とは呼ばない。

予測打点・基準時刻・理想の速度ベクトルは固定する。新しい軸に対して支点、速度の軸方向成分、角速度を生成する。保存された角度実行誤差はyaw後の局所的な傾斜平面へ回転して適用するため、yaw変更で傾斜誤差の量が変わらない。

各計画から開始姿勢を生成し、従来と同じsin²パルスで前向きに実行する。接触後のHLAを回したり、実際の打点へ到達位置を補正したりしない。軌道が変わるので接触探索は必要。衝突応答は従来の3モデルをそのまま使う。

yaw=0では従来の計画・衝突結果と一致する。従来の `plan_tilt_sweep.py` の既定動作も維持する。今回のyaw診断は独立した診断経路であり、Rust本体の能力値・確率・係数・計画は変更しない。

## 出力

ファイル接頭辞は既存と同じ `plan_tilt_`。以下の6ファイルを最初に共有する。

- `plan_tilt_summary.csv`
- `plan_tilt_pairs.csv`
- `plan_tilt_kinematics.csv`
- `plan_tilt_coverage.csv`
- `plan_tilt_checks.csv`
- `plan_tilt_metadata.json`

その他、sample、states、transitions、EV/VLA表も保存する。metadataはversion=2で、scenariosに各候補の傾斜と打者基準yaw、yaws_deg、yaw座標系、kinematicsの指標一覧を保持する。

scenarioは `tilt_15deg_yaw_-10deg` 等。orientationの `saved_tilt` は元の傾斜・yaw=0の対照、`planned_tilt` は候補。対照が候補ごとに繰り返し記載されるので合算しない。`stratum=planned_tilt` は元の保存計画の傾斜帯による層別。

summaryには従来のEV/VLA指標に加え、次のフェア限定指標を出す。

| 指標 | 意味 |
| --- | --- |
| `fair_avg_abs_hla_deg` | フェア打球のHLA絶対値の平均 |
| `fair_avg_batter_hla_deg` | 打席符号を掛けたHLAの平均。フェア範囲内なので±180度の不連続は含まない |
| `fair_central_abs_lt10_fraction` | フェア打球のうちHLA絶対値10度未満の割合 |
| `fair_high_vla_negative_batter_hla` | VLA>=50度かつ打者基準HLA<0のフェア件数 |
| `fair_high_vla_zero_batter_hla` | 同じくHLA=0の件数 |
| `fair_high_vla_positive_batter_hla` | 同じくHLA>0の件数 |

高角度の符号別3件数の合計は `fair_high_vla` に一致する。フェア打球がない平均・割合は空欄。低EV・高EV・下向き・高VLAは従来どおり件数を `fair_contacts` で割って比較する。

kinematicsには `avg_planned_axis_yaw_batter_deg` と `avg_reference_axis_yaw_batter_deg` を追加する。基準点速度、支点の最低高さ、地面下になる件数も確認する。理想速度を共通にしても、接触時刻や軸上の位置が変わるので、実際の接触点速度まで一致するとは限らない。

## 詳細な方向分布

前回の保存結果用監査もversion=2に対応している。

```sh
python3 tools/diagnostics/plan_tilt_direction_audit.py \
  --input-dir ../saberbb_plan_tilt_yaw_sample \
  --output-dir ../saberbb_plan_tilt_yaw_direction_audit
```

これはDB・接触探索なしで、HLA×VLA、予測高さ・左右打席、共通成功/共通フェア集合のHLA変化を集計する。metadataで宣言した傾斜・yawと、保存理想軸の一致も検査する。

同じseedの旧標本と新標本を一度に渡すと重複seedとして拒否する。それぞれ別の出力先へ監査し、標本数や条件の違いを明示して読む。

高角度率・左右の広がり・フェア方向率・EV・支点高さを合わせて判断する。yawごとの発生割合や選手の狙い方は今回導入しない。標本が小さいため、少数の改善で本体の既定値を決めない。

## 上下の狙い打点

[上下狙い打点の分離診断](plan_aim_height_sweep.md)では、予測球中心を維持したまま意図したバット基準点の上下差を指定できる。既定はyawを0に固定し、傾斜0/15度×上下−10/0/+10 mmを同じ1,000投球で比較する。0 mmではこのyaw診断の結果と一致する。
