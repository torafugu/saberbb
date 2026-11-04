# 中間速度と中間狙いで離散化の影響を確認

固定構成比のseed1検証では、4領域の割合は基準内だったが、EV×VLA格子差が0.2555となり0.25を超えた。EV 40〜45 m/sとVLA 0〜30度の不足が目立つため、90%速度と−15 mm狙いを追加して、少数の離散値で代表させた影響を調べる。

これはseed1の残差を見て設定した探索的診断であり、新たな独立検証とは扱わない。元の固定構成比・基準・検証結果は保持し、中間条件の結果で上書きしない。戦略や衝突係数を変更せず、自動的な構成比最適化もしない。

## 比較する4条件

| 条件 | 速度比のスイング構成比 | 上下狙いのスイング構成比 |
| --- | --- | --- |
| frozen_original | 60%速度20%、80%速度20%、100%速度60% | −30 mm 80%、0 mm 20% |
| speed_midpoint_same_mean | 60%速度20%、80%速度10%、90%速度20%、100%速度50% | 元と同じ |
| aim_midpoint_same_mean | 元と同じ | −30 mm 70%、−15 mm 20%、0 mm 10% |
| both_midpoints_same_means | 中間速度あり | 中間狙いあり |

速度と狙いは独立と仮定する。すべての条件で試行重みの平均速度比0.88、平均上下狙い−24 mmを維持する。速度は80/100%から各10ポイントを90%へ移し、狙いは−30/0 mmから各10ポイントを−15 mmへ移す。出力EV/VLAの平均まで一致させる操作ではない。分布の分散・形状は変わるため、純粋な数値計算誤差ではなく、計画分布を粗く代表させた影響の感度確認である。

## 1. 同じseed1標本で結合条件を計算

```sh
python3 tools/diagnostics/plan_speed_sweep.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_plan_midpoint_sample_seed1 \
  --sample-size 1000 \
  --sample-seed 1 \
  --exclude-sample ../saberbb_plan_speed_aim_sample/plan_tilt_sample.csv \
  --tilts-deg 15 \
  --speed-ratios 0.6 0.8 0.9 1 \
  --aim-z-mm -30 -15 0
```

元と同じ観測更新入力・seed・除外CSVなら、直前のholdout seed1と同じ1,000投球になる。標本は結果によらず選ぶ。4速度×3狙い＋保存計画対照で最大13,000探索。既存端点も同じ計算で再確認する。SABERBB_DBまたは--dbを読み取り専用で使い、選手・試合の再生成は不要。

## 2. 宣言した4構成比だけを比較

```sh
python3 tools/diagnostics/plan_speed_mixture_compare.py \
  --input-dir ../saberbb_plan_midpoint_sample_seed1 \
  --statcast-csv ../saberbb_finite_statcast_input/savant_data.csv \
  --output-dir ../saberbb_plan_midpoint_comparison_seed1 \
  --mixtures-json tools/diagnostics/plan_midpoint_mixtures.json \
  --tolerances-json tools/diagnostics/plan_speed_aim_frozen_tolerances.json
```

この段階はDB・探索・衝突再計算なし。自動の構成比格子は生成せず、12単独条件＋4固定構成比を出力する。許容幅は前回と同じ参考基準で、途中で緩めない。構成比JSONのハッシュもmetadataへ記録する。

summaryには既存4領域に加え、以下の割合とStatcastの対応値を追加する。区間はいずれも下端を含み、上端を含まない。

| 列の接頭辞 | 領域 |
| --- | --- |
| ev_40to45 | EV 40〜45 m/s |
| ev_25to35 | EV 25〜35 m/s |
| vla_0to30 | VLA 0〜30度 |
| vla_neg60to_neg30 | VLA −60〜−30度 |
| vla_40to50 | VLA 40〜50度 |

末尾_fractionがシミュレーション、statcast_で始まる列が実測。all_successfulとfair_directionは別集団であり、実測比較にはfair_directionを使う。追加領域は残差の説明用で、固定許容幅の合否項目には自動追加しない。

frozen_originalが前回の固定構成比を再現することを先に確認し、速度のみ・狙いのみ・両方の順に、残差領域、格子差、既存4領域、接触/フェア方向率を見る。中間条件が良くても、そのまま実際の選択頻度とは見なさない。後で採用候補を決めた場合は、別の未使用標本で改めて固定検証する。

## 共有する6ファイル

手順2の出力先から以下を共有する。

- `plan_speed_mixture_summary.csv`
- `plan_speed_mixture_assessment.csv`
- `plan_speed_mixture_checks.csv`
- `plan_speed_mixture_metadata.json`
- `plan_speed_mixture_ev_vla.csv`
- `statcast_coverage.csv`

今回は残差を比較するため、envelopeの代わりにEV×VLA格子を共有する。大きい元states JSONLの共有は不要。
