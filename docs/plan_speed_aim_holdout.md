# 固定した構成比を別の投球標本で再検証

seed0で得られた代表例の構成比と参考基準を固定し、seed1の別標本で比較する。新しい標本で構成比・係数・許容幅を調整しない。打者戦略の実装や実際の選択頻度の推定とは分けた検証である。

固定条件は傾斜15度、yaw 0度。速度構成比は通常速度60%、80%速度20%、60%速度20%。上下狙いは−30 mmが80%、中心が20%。速度と狙いは独立と仮定し、結合重みをplan_speed_aim_frozen_mixture.jsonへ保存した。

| 速度比 | −30 mm狙いの試行重み | 中心狙いの試行重み |
| --- | --- | --- |
| 1.0 | 0.48 | 0.12 |
| 0.8 | 0.16 | 0.04 |
| 0.6 | 0.16 | 0.04 |

## 1. 元標本を除外して接触を計算

```sh
python3 tools/diagnostics/plan_speed_sweep.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_plan_speed_aim_holdout_seed1 \
  --sample-size 1000 \
  --sample-seed 1 \
  --exclude-sample ../saberbb_plan_speed_aim_sample/plan_tilt_sample.csv \
  --tilts-deg 15 \
  --speed-ratios 0.6 0.8 1 \
  --aim-z-mm -30 0
```

seedだけを変えると投球が重なる可能性があるため、前回のsample CSVに含まれるgame_id/count_seqを、ハッシュ順位を付けて選ぶ前に除外する。除外キーは今回のvariantの母集団内に存在する必要があり、重複・空・未知キーは拒否する。元標本は同じ観測更新データから生成したものを使う。

6条件と保存計画対照で最大7,000探索。時計不成立は全条件で未評価。SABERBB_DB（または--db）を読み取り専用で使う。選手・試合の再生成は不要。

sampling_exclusionに除外元CSVのパス・SHA256・除外キーを記録する。population_trialsは除外後の母集団数、population_trials_before_exclusionは除外前の数。キャッシュ診断でも、保存された標本と除外キーが重ならないことを確認する。除外CSVを後で移動してもキャッシュ診断には不要。従来のコマンドは--exclude-sample未指定なら同じ標本を選ぶ。

## 2. 固定構成比と固定基準で比較

```sh
python3 tools/diagnostics/plan_speed_mixture_compare.py \
  --input-dir ../saberbb_plan_speed_aim_holdout_seed1 \
  --statcast-csv ../saberbb_finite_statcast_input/savant_data.csv \
  --output-dir ../saberbb_plan_speed_aim_holdout_comparison_seed1 \
  --mixtures-json tools/diagnostics/plan_speed_aim_frozen_mixture.json \
  --tolerances-json tools/diagnostics/plan_speed_aim_frozen_tolerances.json
```

追加探索・DB読み取りなし。自動の構成比格子は生成しない。単独6条件に加え、mixture:seed0_frozen_tilt15_speed60_20_20_aim80_20の1条件を比較する。検証の対象はこのmixture行で、単独条件から良いものを選び直さない。

固定した参考基準は低EV率±1ポイント、高EV率±5ポイント、下向き率±5ポイント、高角度率±3ポイント、格子total variation≤0.25。seed0では事後的に使った診断基準であり、実証済みの許容値や信頼区間ではない。今回の新標本では結果を見る前に固定する。構成比と基準ファイルのハッシュも比較metadataへ保存する。

seed0の同じ構成比では、低EV2.77%、高EV23.57%、下向き32.07%、高角度12.44%、格子差0.226。これらは丸めた参考値で、再検証では同じStatcast CSVへの差と指定基準を使う。seed0を同じ固定ファイルで再集計したければ手順2のinput-dirを元のsaberbb_plan_speed_aim_sampleに変更し、別の新しいoutput-dirで実行できる。

元標本との投球重複はなくなるが、同じ選手・試合が含まれ得る。同じシミュレーション母集団での安定性確認であり、独立したシーズンや別の選手集団での外部検証ではない。Statcastの対象集団・バント・欠損などの制約も残る。結果が基準を外れた場合も、その場で基準や重みを変えず、差を記録する。

## 共有する6ファイル

手順2の出力先から以下を共有する。

- `plan_speed_mixture_summary.csv`
- `plan_speed_mixture_assessment.csv`
- `plan_speed_mixture_envelope.csv`
- `plan_speed_mixture_checks.csv`
- `plan_speed_mixture_metadata.json`
- `statcast_coverage.csv`

ファイル名が以前と同じなので、出力先がsaberbb_plan_speed_aim_holdout_comparison_seed1であることを確認する。
