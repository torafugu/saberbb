# 両方の中間値を含む固定候補のseed2検証

seed1の残差を見て設定した中間値比較では、both_midpoints_same_meansのEV×VLA格子差が0.2346となった。これは探索結果なので、その候補だけを固定し、seed0とseed1の両方を除いた新しい1,000投球で再検証する。今回の結果を見て構成比・許容幅・衝突係数を調整しない。

## 固定する条件

傾斜15度、yaw 0度。速度と上下狙いは独立と仮定し、以下の試行構成比を掛け合わせた12条件を`tools/diagnostics/plan_midpoint_frozen_mixture.json`に固定した。既存の4候補設定からboth_midpoints_same_meansをそのまま取り出しており、他候補を再選択しない。

| 速度比 | 試行構成比 |
| --- | --- |
| 0.6 | 20% |
| 0.8 | 10% |
| 0.9 | 20% |
| 1.0 | 50% |

| 上下狙い | 試行構成比 |
| --- | --- |
| −30 mm | 70% |
| −15 mm | 20% |
| 0 mm | 10% |

平均速度比0.88、平均上下狙い−24 mm。接触後の打球構成比ではなく、投球ごとの計画の構成比である。実際の選択頻度を推定したものではない。ゲーム内のランダム選択は追加せず、将来の打者戦略とは別に物理モデルの表現範囲を確認する。

## 1. 過去の2標本を除外して再計算

```sh
python3 tools/diagnostics/plan_speed_sweep.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_plan_midpoint_holdout_seed2 \
  --sample-size 1000 \
  --sample-seed 2 \
  --exclude-sample ../saberbb_plan_speed_aim_sample/plan_tilt_sample.csv \
  --exclude-sample ../saberbb_plan_midpoint_sample_seed1/plan_tilt_sample.csv \
  --tilts-deg 15 \
  --speed-ratios 0.6 0.8 0.9 1 \
  --aim-z-mm -30 -15 0
```

`--exclude-sample`を繰り返し指定し、その合併に含まれるgame_id/count_seqを標本選択前に除外する。各CSV内の重複・空・負値、および入力母集団にないキーは拒否する。異なる除外CSV間の重なりは一度だけ数える。単一指定の従来コマンド、未指定の従来標本選択も維持する。

同じ観測更新入力を使い、seed0・seed1が各1,000投球で重ならない場合、除外数は2,000。metadataのpopulation_trialsは除外後、population_trials_before_exclusionは除外前の母集団数。複数指定時はsampling_exclusion.manifestsに各CSVのパス・SHA256・キー・件数を保存し、sampling_exclusion直下に合併キー・件数を保存する。キャッシュ診断も各除外元の合併との一致と新標本との非重複を確認する。元CSVを移動しても、保存後の比較時には読み直さない。

4速度×3狙いと保存計画対照で最大13,000探索。時計不成立は全条件で未評価。SABERBB_DB（または--db）のDBを読み取り専用で利用し、選手・試合の再生成は不要。この手順が接触を再計算する段階である。

## 2. 固定候補1件を同じ参考基準で評価

```sh
python3 tools/diagnostics/plan_speed_mixture_compare.py \
  --input-dir ../saberbb_plan_midpoint_holdout_seed2 \
  --statcast-csv ../saberbb_finite_statcast_input/savant_data.csv \
  --output-dir ../saberbb_plan_midpoint_holdout_comparison_seed2 \
  --mixtures-json tools/diagnostics/plan_midpoint_frozen_mixture.json \
  --tolerances-json tools/diagnostics/plan_speed_aim_frozen_tolerances.json
```

DB読み取り・探索・衝突再計算なし。単独12条件と固定構成比1件を出力する。判定対象はfair_directionの`mixture:both_midpoints_same_means`行。単独条件の中から良いものを選び直さない。比較metadataには固定構成比・参考基準・入力のハッシュを記録する。

参考基準は前回と同じ低EV率±1ポイント、高EV率±5ポイント、下向き率±5ポイント、高角度率±3ポイント、EV×VLA格子total variation≤0.25。いずれもStatcastとの差に対する暫定的な診断基準であり、実証済みの許容誤差・信頼区間ではない。追加の残差領域の割合は説明用で、合否項目には加えない。

seed1では低EV2.93%、高EV21.33%、下向き31.62%、高角度12.20%、格子差0.2346だった。今回も同じStatcast CSVと固定基準で評価する。基準を満たしても、同じシミュレーション母集団の別投球に対する安定性確認までである。同じ選手・試合は含まれ得る。実測との対象集団の違い、バント不明、EV/VLA欠損、fair_directionが最終的なフェア打球判定ではない制約も残る。基準を外れた場合は差を記録し、結果に合わせてこの固定候補や基準を変更しない。

## 共有する6ファイル

手順2の`saberbb_plan_midpoint_holdout_comparison_seed2`から共有する。

- `plan_speed_mixture_summary.csv`
- `plan_speed_mixture_assessment.csv`
- `plan_speed_mixture_checks.csv`
- `plan_speed_mixture_metadata.json`
- `plan_speed_mixture_ev_vla.csv`
- `statcast_coverage.csv`

ファイル名が前回と同じなので、seed2の出力先を確認する。大きいstates JSONLの共有は不要。

固定候補を変更せず投球標本の変動幅を調べる場合は、[投球単位ブートストラップ](plan_mixture_bootstrap.md)を使う。保存済みキャッシュだけで実行できる。
