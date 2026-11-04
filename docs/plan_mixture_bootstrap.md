# 固定候補の投球単位ブートストラップ

seed2では両方の中間値を含む固定候補の格子差は0.2394で基準内だったが、高角度率は14.39%となり、実測との差+3.77ポイントが参考基準±3ポイントを超えた。混合比率・係数・判定基準を変更せず、保存済みキャッシュから標本の変動幅を調べる。元のseed2の基準外判定は保持する。

## 実行

[seed2固定検証](plan_midpoint_holdout.md)で生成した投球別キャッシュと同じStatcast CSVを使う。集計CSVだけでは同じ投球内の条件間対応が失われているため、input-dirにはsummaryの比較出力先ではなく、states JSONLを含む元の計算出力先を指定する。

```sh
python3 tools/diagnostics/plan_speed_mixture_bootstrap.py \
  --input-dir ../saberbb_plan_midpoint_holdout_seed2 \
  --statcast-csv ../saberbb_finite_statcast_input/savant_data.csv \
  --output-dir ../saberbb_plan_midpoint_bootstrap_seed2 \
  --mixtures-json tools/diagnostics/plan_midpoint_frozen_mixture.json \
  --tolerances-json tools/diagnostics/plan_speed_aim_frozen_tolerances.json \
  --replicates 2000 \
  --bootstrap-seed 0
```

SABERBB_DBの設定や選手・試合の再生成は不要。DB読み取り・接触探索・衝突再計算なし。Python標準ライブラリだけで動作する。キャッシュを一度検証・読み込みし、投球ごとの疎な格子寄与に集約してから再標本化する。出力先は未使用のディレクトリを指定する。

## 計算方法

1. 既存のキャッシュ監査で標本・除外記録・時計・全条件・ベクトル・集計を検証する。
2. 明示JSONにある固定候補1件だけを読み込む。自動格子や候補選択は行わない。
3. 各投球の12条件を固定の試行重みで加重する。各条件を乱数で選ぶことはない。空振り・接触失敗・時計不成立の投球も標本に残す。
4. n投球から復元抽出でn回引く操作を2,000回繰り返す。同じ投球の全条件に、同じ抽出回数を適用する。条件同士を独立に抽出しない。
5. 各回の評価可能投球数を分母に接触率・フェア方向率を求める。EV/VLAの割合はフェア方向の成功打球の加重寄与を合計してから正規化する。時計不成立を空振りと数えない。
6. Statcastの分布を固定して4領域の差（ポイント）とEV×VLA格子差を求める。各指標の2.5/50/97.5パーセンタイルと、元の参考基準内に入った再標本の割合を出力する。

ゲーム内の乱数は使わず、診断専用の固定seedを使う。同じ入力・設定・Python乱数実装なら再現可能。元標本の点推定は既存の混合比較の格子寄与・接触率を再構築して検査し、同じ推定量を使う。

## 出力の読み方

`plan_speed_mixture_bootstrap_summary.csv`の行は以下。

| metric | point_estimateとパーセンタイルの単位 | 判定対象 |
| --- | --- | --- |
| ev_lt25_delta_pp | 実測との差、パーセントポイント | 低EV率±1ポイント |
| ev_ge45_delta_pp | 同上 | 高EV率±5ポイント |
| vla_lt0_delta_pp | 同上 | 下向き率±5ポイント |
| vla_ge50_delta_pp | 同上 | 高角度率±3ポイント |
| grid_total_variation | 0〜1の分布差 | ≤0.25 |
| geometry_contact_probability | 0〜1の割合 | 補助指標、基準なし |
| fair_probability_per_evaluable_trial | 0〜1の割合 | 補助指標、基準なし |

4領域のsimulation_point_fractionとstatcast_reference_fractionは、差にする前の割合（0〜1）。percentile_2_5/median/percentile_97_5はpoint_estimateと同じ単位。within_tolerance_fractionは基準内の再標本数÷全反復数。metadataのall_within_tolerance_fractionは設定された基準をすべて同時に満たした再標本の割合。

フェア方向の成功打球がない再標本は形状指標を空欄にする。valid_replicatesとundefined_replicatesを出し、有効値だけからパーセンタイルを計算する。基準内割合の分母には全反復を残すため、空の打球集団は合格にならない。時計がすべて不成立の再標本では接触率・フェア方向率も空欄。

まず点推定がseed2の比較を再現することを確認し、次に高角度率の差の変動幅を見る。±3ポイントの境界をまたぐかどうかは安定性の説明材料になるが、元の基準外判定を合格へ変更する規則には使わない。格子差と他の領域も同時に確認し、良い指標だけを選ばない。

## 解釈の制約

- これは現在の標本を元にした条件付きの近似的な変動幅。Statcast側の標本誤差は再標本化していない。欠損・推定値・対象集団の違いも含めて評価した区間ではない。
- 投球を交換可能とみなす。同じ選手・試合内の依存や、元の有限母集団から非復元抽出した補正は扱わない。試合・選手を単位とした不確実性まで保証しない。
- 条件間の相関は同じ投球をまとめて抽出して保持する。12条件や2,000回の再標本化を、新たな独立観測の追加とは数えない。
- 95%パーセンタイル範囲は合否の新基準や校正済みの同等性検定ではない。格子TVの範囲もバイアス補正済みの区間・帰無分布・有意差検定ではない。
- 基準内の再標本割合は、真のモデルが許容可能である確率ではない。この診断だけで偏りの恒常性や物理的原因を確定できない。
- fair_directionは最終的なフェア打球分類ではない。物理モデルの適格条件、バント、リーグ・選手構成など元の比較の制約も残る。

## 共有するファイル

今回の出力先`saberbb_plan_midpoint_bootstrap_seed2`から次の4ファイルを共有する。

- `plan_speed_mixture_bootstrap_summary.csv`
- `plan_speed_mixture_bootstrap_checks.csv`
- `plan_speed_mixture_bootstrap_metadata.json`
- `statcast_coverage.csv`

全反復の値は`plan_speed_mixture_bootstrap_replicates.csv`にも保存するが、最初の結果確認では共有不要。大きい元states JSONLの共有も不要。
