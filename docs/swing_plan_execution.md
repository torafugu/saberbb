# スイング計画と実行ずれの分離

前回の診断では、effort=0.85で低EVが増えた一方、強い打球が大幅に減った。また、effortを変えるたびに目標位置へ到達するよう開始姿勢も変わっていた。この動作を「意図して弱く振る計画」として残し、実行時の加速不足や開始遅れは、同じ計画の開始姿勢から積分するように分離する。

## 実行

```sh
python3 tools/diagnostics/swing_execution_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_plan_execution \
  --variant observed_sigma_0.005_r0.9 \
  --gyro-sign 0 \
  --plan-efforts 1 0.85 \
  --execution-ratios 1 0.85
```

`SABERBB_DB` は元データのDBを指定。試合や選手の再生成は不要。4条件を比較するため、時計条件の成立する投球ごとに4回接触を探索する。一定角速度側は保存結果を使い、計画どおりの加減速実行は対照と実行ずれ比較で再利用する。実ユーザーデータでの所要時間は未測定。

| 計画の強さ | 実行比率 | 意味 |
| --- | --- | --- |
| 1 | 1 | フルスイング計画を計画どおり実行 |
| 0.85 | 1 | 弱く振る計画を計画どおり実行 |
| 1 | 0.85 | フルスイングの開始姿勢から加速不足で実行 |
| 0.85 | 0.85 | 弱く振る計画にも加速不足が発生 |

計画の強さは `--plan-efforts`（従来の `--efforts` も同義）、実行比率は `--execution-ratios`。両方0より大きく1以下。発生割合は導入しない。これらは明示的な感度条件で、実際の打者の計画/失敗確率ではない。

開始遅れだけを見る場合:

```sh
python3 tools/diagnostics/swing_execution_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_execution_delay \
  --plan-efforts 1 \
  --execution-ratios 1 \
  --start-offset-ms 0 5
```

既定は計画1/0.85、実行比率1、遅れ0なので、前回の意図的なeffort比較を維持する。開始遅れは正で遅い、負で早い。実行比率1かつ遅れ0の計画対照が指定に含まれない場合は自動追加し、metadataにrequested_scenariosとadded_plan_controlsを記録する。

## 状態の分離と運動

`SwingPlan` は保存予測計画と意図したeffortから、開始時刻・開始支点・開始軸・ピーク並進速度/角速度を一度だけ生成する。予測打点と基準時刻の情報は計画に使うが、実際の接触結果から計画を選ばない。計画effortを変えた場合は異なる開始姿勢になり、計画どおりに実行すれば共通の基準位置・軸に到達する。

`ExecutionDeviation` はacceleration_ratioとstart_delay_secだけを保持する。`PlanExecutionMotion` は計画した開始姿勢を変えずに、速度パルスの振幅と開始時刻を変える。ratioは加速度/速度の振幅を同じ比率で縮めるモデルで、速度曲線の形や所要時間は今回固定。

パルスの始点からの積分をF、保存基準時刻での積分をF0、計画のピーク角速度をomega_p、計画支点速度をvp_pとすると、開始状態は

- `a_start = rotate(a_ref, omega_p, -F0)`
- `pivot_start = pivot_ref - vp_p*F0`

実行比率r・開始ずれdに対して、qを開始ずれ込みの時刻として

- `a(q) = rotate(a_start, omega_p*r, F(q))`
- `pivot(q) = pivot_start + vp_p*r*F(q)`

で前向きに積分する。実行時にはF0を引き直さず、目標位置への平行移動や角度補正もしない。r=1なら従来の意図的effortモデルと一致し、r<1なら同じ開始姿勢から基準時刻の位置・軸がずれる。開始遅れも位置と速度の両方へ一度だけ適用する。

点速度は位置の時間微分と一致するベクトル式で計算。接触時の瞬間角速度を衝突応答に渡す。計画/実行ともにsin²パルスの仮定であり、実測の身体運動ではない。Rust本体の試合結果・能力値・衝突係数は変更しない。

## 出力

従来の `swing_execution_summary/pairs/transitions/ev_vla` は保存済み一定角速度との比較として維持する。scenario名は `plan_1_execution_0.85_anchor_0.5_delay_0ms` のように計画と実行を明記する。metadataはversion=2。

実行ずれの条件がある場合は、次の4ファイルを追加する。

- `swing_execution_deviation_summary.csv`
- `swing_execution_deviation_pairs.csv`
- `swing_execution_deviation_transitions.csv`
- `swing_execution_deviation_ev_vla.csv`

こちらの `orientation=planned` は**同じ計画をratio=1/遅れ0で実行した対照**、`executed` は実行ずれを加えた結果。pairsのbefore/afterも同じ対応。一定角速度と比較した差と、同じ計画に対する実行誤差の差を混同しない。

`swing_execution_kinematics.csv` に、同じ計画の基準時刻での位置ずれ・軸角度差・点速度の平均をscenario/scopeごとに記録する。分母は時計が成立するevaluable_trialsで、モデルや接触の採用に重複させない。時計不成立の値を0として埋めない。

従来summaryのproxy_pivot_z_mは保存基準姿勢の支点座標で、開始時刻や実行ずれ後の支点高さそのものではない。新しい開始支点はstatesのplanを参照する。

`states.jsonl` の有効行にはplan（開始姿勢を含む）、execution_deviation、実際の速度振幅を記録するexecutionを保存する。execution.effortは計画effort×実行比率の積で、意図そのものの値とは限らない。無効行にも計画effort/実行比率を分けて記録する。

計画開始時刻と実際の開始時刻の両方が、計画に使用した観測より後であることを要求する。時計が成立しなければ前回と同じ未評価理由で保持。接触率の比較は同じ投球を除外した `contact_fraction_of_evaluable_trials` を使い、時計不成立を空振りとみなさない。EV等は各条件の成功例、共通集合はpairsで確認する。その他の分母・端部/曲線範囲外の扱いは[既存診断](swing_execution_probe.md)と同じ。

実行後は通常summary、deviationのpairs/transitions/ev_vla、kinematics、checks、coverage、metadataを共有する。実データの影響を確認してから、計画・実行ずれをどの条件で発生させるかを決める。

## 保存結果の混合比較

実行比率別の保存結果が揃ったら、[実行状態の混合とStatcast比較](swing_execution_mixture_compare.md)で任意の試行重みを評価できる。接触探索の再実行やDB指定は不要。通常実行と各失敗状態の単独結果も残し、選択後の打球構成比を監査する。付属の混合比は感度比較用の仮定で、ゲームの発生確率には反映しない。
