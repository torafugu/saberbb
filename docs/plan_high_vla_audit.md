# 高角度打球の固定構成比分解と狙いの対応比較

seed2の固定候補では高角度率が14.39%で、実測10.62%との差+3.77ポイントが参考基準を超えた。投球単位ブートストラップでも高角度率の過剰が残るため、構成比・係数・許容幅を変えず、どの計画・接触条件から高角度打球が出ているか調べる。

## 実行

```sh
python3 tools/diagnostics/plan_high_vla_audit.py \
  --input-dir ../saberbb_plan_midpoint_holdout_seed2 \
  --output-dir ../saberbb_plan_high_vla_seed2 \
  --mixtures-json tools/diagnostics/plan_midpoint_frozen_mixture.json
```

input-dirはstates JSONLを含むseed2の計算キャッシュであり、混合比較やブートストラップの集計出力先ではない。SABERBB_DBやStatcast CSVは不要。Python標準ライブラリのみ。DB読み取り・接触探索・衝突再計算なし。既存キャッシュ監査に加えて接触幾何・局所接触座標・保存インパルスと出射速度の整合性を確認する。既存出力先への上書きは拒否する。

## 1. 速度×狙いごとの寄与

`plan_high_vla_summary.csv`には固定候補全体の確率とフェア方向打球内の割合を出す。geometry/fairのprobability_per_evaluable_trialは評価可能投球あたりの確率。high_vlaなど6領域には、これに加えてfraction_among_mixture_fairを出す。高角度はVLA≥50度、低EVはEV<25 m/s、高EVはEV≥45 m/s、下向きはVLA<0度。残差領域VLA 0〜30度・EV 40〜45 m/sは下端を含み上端を含まない。

`plan_high_vla_components.csv`の12行は、速度×狙いごとの試行重みと寄与を示す。

| 列 | 意味 |
| --- | --- |
| trial_weight | 固定した試行構成比。成功打球の構成比ではない |
| fair / high_vla | その条件の実件数。high_vlaはフェア方向の高角度打球のみ |
| fair_contribution_fraction | 混合全体のフェア方向打球のうち、その条件が占める加重割合 |
| high_vla_fraction_among_band_fair | その条件・区分内のフェア方向打球に占める高角度打球の割合 |
| high_vla_contribution_pp | 混合全体の高角度率への寄与、パーセントポイント |
| high_vla_share_of_mixture_high | 混合全体の高角度打球のうち、その条件が占める加重割合 |
| low_ev/high_ev/downward/vla_0to30/ev_40to45_contribution_pp | 他の領域への同じ定義の寄与 |

評価可能投球数をN、条件iの重みをw_i、フェア方向件数をF_i、高角度件数をH_iとすると、混合フェア確率P_F=Σw_i F_i/N、高角度率への寄与=100 w_i H_i/N/P_F。寄与を合計すると、既存の混合比較と同じ高角度率になる。空のフェア集団の条件内割合や、高角度件数が全体でゼロの場合の高角度構成比は空欄で、ゼロと区別する。

これはシミュレーション内の打球を分解したもの。実測側には同じ戦略ラベルがないため、「実測との差+3.77ポイントを各条件に配分した結果」ではない。寄与が大きいだけで、その条件が誤りとは判定しない。

## 2. 投球高・接触位置による分解

`plan_high_vla_strata.csv`は同じ12条件を以下で区分する。

| stratum | band_1 / band_2 |
| --- | --- |
| forecast_height | 予測投球高（m） / all |
| normal_elevation | 接触法線の仰角（度） / all |
| signed_impact | 局所座標の符号付き接触位置 b/r / all |
| abs_axial_fraction | バット軸方向の接触位置の絶対値÷半長 / all |
| contact_region | キャッシュの接触部位 / all |
| height_normal | 予測投球高 / 接触法線仰角 |

bandは下端を含み上端を含まない。接触がない場合や局所座標が定義できない場合もundefined/no_entry区分に保持する。時計不成立はclock_invalidに数え、空振りには加えない。geometry_probability_in_bandとfair_probability_in_bandの分母はその条件・区分の評価可能投球、寄与列の分母は全体の混合フェア確率である。

**同じstratumの行だけを合計する。異なるstratumの行を足すと同じ打球を重複計上する。** 各stratumの寄与の合計が全体を再現することをchecksで検証する。

予測投球高はスイング前に使える情報。接触法線・接触位置・接触部位は事後情報なので、その値で狙いを決める実装には使わない。

## 3. 同じ投球で狙いだけを変えた比較

`plan_high_vla_pairs.csv`は、同じ傾斜・yaw・速度で、0→−15 mm、0→−30 mm、−15→−30 mmの3比較を出す。4速度×3比較の全体行と、予測投球高別の行になる。方向は常にafter−before。固定候補に含まれる条件間だけを比較し、混合比率を使った再重み付けや最適戦略の選択はしない。

- `*_probability_delta_pp`：同じ評価可能投球を分母にした発生率の差。high_vlaは「全評価可能投球あたりのフェア方向高角度打球」であり、条件付き高角度率の差とは異なる。
- `before/after_high_vla_fraction_among_fair`：それぞれの狙いで得られたフェア方向打球内の高角度率。対象打球の入れ替わりを含む。
- `geometry_gained/lost`、`fair_gained/lost`：狙い変更で接触・フェア方向成功を得た／失った投球数。
- `common_fair`：両方の狙いでフェア方向に成功した共通投球数。
- `common_fair_high_gained/lost`と`common_fair_high_delta_pp`：共通投球で高角度になった／高角度でなくなった件数と、その差をcommon_fairで割ったポイント差。
- `common_fair_mean_delta_ev_ms/vla_deg`：共通投球内の平均EV/VLAの変化。common_fairがゼロなら空欄。

全投球の発生率変化と共通フェア投球内の変化を併せて読み、接触する投球の選別効果と打球の変化を区別する。共通フェア集団だけを見て全投球への効果と解釈しない。詳しい入れ替わりは`plan_high_vla_transitions.csv`に保存する。

これらは現行物理モデル内の反実仮想比較であり、実際の野球の因果効果や最適戦略を推定したものではない。最終的なフェア判定・適格接触・選手／試合の依存など元の診断の制約も残る。新しい合否判定は追加しない。

## 共有する6ファイル

出力先`saberbb_plan_high_vla_seed2`から次を共有する。

- `plan_high_vla_summary.csv`
- `plan_high_vla_components.csv`
- `plan_high_vla_strata.csv`
- `plan_high_vla_pairs.csv`
- `plan_high_vla_checks.csv`
- `plan_high_vla_metadata.json`

transitionsは詳細確認が必要になった場合に使う。大きい元states JSONLやStatcast CSVの再共有は不要。
