# 保存済み傾斜比較のHLA・高さ・左右打席別集計

0/15度の計画で高VLA打球が増える一方、左右速度の広がりが縮む傾向がある。保存済みの接触状態と応答から、HLAとVLAの分布、中央方向への集中、予測打点高さ・左右打席との関係を調べる。

## 実行

seed 0とseed 1の保存フォルダをまとめて指定できる。

```sh
python3 tools/diagnostics/plan_tilt_direction_audit.py \
  --input-dir ../saberbb_plan_tilt_sample ../saberbb_plan_tilt_sample_seed1 \
  --output-dir ../saberbb_plan_tilt_direction_audit
```

一方だけなら `--input-dir` にそのフォルダだけを渡す。出力先は新しいフォルダを指定する。DB、元の観測更新出力、`SABERBB_DB` は不要。接触探索も衝突応答の再計算も行わない。実データでの所要時間は未測定で、保存JSONLの大きさに依存する。

各入力フォルダには次の6ファイルが必要。

- `plan_tilt_metadata.json`
- `plan_tilt_sample.csv`
- `plan_tilt_states.jsonl`
- `plan_tilt_summary.csv`
- `plan_tilt_ev_vla.csv`
- `plan_tilt_kinematics.csv`

大きなstates JSONLを共有する必要はなく、ローカルで集計する。既存の計画傾斜診断が保存したファイルをそのまま使う。

## 入力の検査

metadataの条件、元の入力ハッシュ、標本キーと選択ハッシュを確認し、全投球に全シナリオが一度ずつあることを検査する。statesは投球ごとの連続したレコード群を前提にし、同じ投球が離れた位置に再出現した場合もエラーにする。シナリオの並び順や投球群の並び順自体は問わない。

保存された最終速度ベクトルからEV/HLA/VLAを再計算して照合する。元のoverall/all_swings summaryの全件数、平均EV/VLA/左右速度絶対値、EV/VLAヒストグラムの全セル・分母・割合、kinematicsの件数と平均を照合する。繰り返し記載された保存傾斜対照は一致を検査し、一度だけ集計する。検査がすべて通った後に出力先を作成する。

異なるseedは `dataset=seed:0` / `seed:1` として別々に出力する。標本が重なる可能性があるため、seed間の件数を合算したり独立標本として有意差を判定したりしない。重複seedや元データ・実行条件が異なる入力は拒否する。候補角度の組合せが異なる入力は比較可能。

元のDBや観測出力を読み直さず、保存metadataとmanifestを信頼して集計する診断であり、標本が母集団ハッシュ順の上位であることを再選択して証明するものではない。

## 座標と層別

world HLAは `atan2(vx, vy)`、VLAは `atan2(vz, hypot(vx, vy))`、上下軸はworld z。

保存された理想バット軸のx成分から左右打席を推定する。Rustの定義に合わせ、符号+1をright、-1をleftとする。DBから左右打席を独立に再取得する処理はない。打者基準の角度は `batter_relative_hla = handedness_sign × world_hla` とし、投球ごとに反転してから集計する。正負を引っ張り/流し打ちとは呼ばない。

| stratum | handedness | forecast_height_band_m |
| --- | --- | --- |
| `overall` | all | all |
| `handedness` | right / left | all |
| `forecast_height` | all | 予測打点高さの帯 |
| `handedness_height` | right / left | 予測打点高さの帯 |

高さは予測打点のworld zで、境界は0/0.3/0.6/0.9/1.2/1.5 m。範囲外も保持する。実際の通過高さや、打者のストライクゾーンで正規化した高さではない。

保存された時計不成立行にはrecorded_batting_onlyへの所属情報がないため、今回のscopeはall_swingsのみ。モデル・stratum・cohort・角度座標には重複があるので合算しない。

## 出力

| ファイル | 内容 |
| --- | --- |
| `plan_tilt_direction_selection.csv` | 各層の試行・時計不成立・接触・成功・フェア/ファウル・未定義HLA・端部等の除外件数、支点地面下の監査 |
| `plan_tilt_direction_summary.csv` | EV/VLA・左右速度・HLA絶対値の平均、中央方向割合、円周平均、低EV/高VLA等の件数 |
| `plan_tilt_direction_histogram.csv` | world/打者基準それぞれのHLA×VLA分布。各層内の分母・割合を記録 |
| `plan_tilt_direction_pairs.csv` | 同じ投球の共通成功/共通フェア集合におけるHLA・中央方向割合の前後比較 |
| `plan_tilt_direction_transitions.csv` | 保存傾斜対照から候補へ、中央・正負のフェア方向・ファウル・未評価等の遷移 |
| `plan_tilt_direction_checks.csv` | 保存ベクトルと角度・速度、元集計・kinematicsとの照合誤差 |
| `plan_tilt_direction_metadata.json` | 各標本の条件、入力SHA256、角度・高さの境界、座標系、検査と限界 |

`scenario=saved_tilt` が対照、`tilt_0deg` 等が候補。以前のように対照を候補ごとに重複出力しない。

## 読み方

最初に `model=finite_I0.00015_et0.4`、`stratum=overall`、`cohort=fair_direction` を比較する。`all_successful` はファウル・後方を含む別集団なので、フェア内の広がりと混同しない。

- `central_abs_lt5/10/20_fraction` は、定義されたHLAのうち絶対値が5/10/20度未満の割合。中央への集中を直接確認する。
- `avg_abs_hla_deg` も定義されたHLAを分母にする。定義数は `defined_hla_contacts` を参照する。
- ±180度付近を単純平均すると0度に見えるため、符号付き平均には円周平均を使う。方向が相殺して平均方向を定めにくい場合は空欄。`world_hla_resultant_length` は円周上の集中の強さで、1に近くても中央方向への集中とは限らない。
- HLAが未定義の真上・真下の打球を0度で埋めない。histogramでは `hla_band_deg=undefined` として残す。ヒストグラムの分母は未定義HLAを含むcohortの全打球数なので、中央方向割合とは分母が異なりうる。
- フェア方向は従来どおり `abs(HLA)<45`。ちょうど±45度はファウル方向。中央方向は `abs(HLA)<10` などの記述的な帯で、実測データに校正した打ち分け区分ではない。
- summaryの低EV/高EV/下向き/高VLAは件数。各cohortの `contacts` で割る。支点地面下の割合はselectionの `pulse_pivot_below_ground/evaluable_trials` を使う。
- pairsの `common_successful` は両条件で成功、`common_fair` はさらに両条件でフェア方向の同じ投球。HLA差は±180度を折り返した最短の角度差の絶対値。transitionsの正負はworld HLA基準。

HLAが中央に集まることでフェア方向率が上がっているのか、高VLAの増加が特定の予測高さ・左右打席に偏っていないかを確認する。これだけで意図した打ち分け、身体の実行可能性、実際の打球方向分布との一致を保証しない。Statcastへの自動フィットや本体の係数変更は行わない。

実行後はsummary、selection、histogram、pairs、checks、metadataの6ファイルを共有する。遷移を詳しく見る場合はtransitionsも共有する。

## yawを含む保存計画

[傾斜×yaw診断](plan_tilt_yaw_sweep.md)のmetadata version=2も読める。scenario名はmetadataのscenariosから取得し、宣言した傾斜・yawと保存理想軸の一致を検査する。旧version=1のファイルも従来どおり読める。同じseedのフォルダを複数渡すことはできないため、旧標本と新しいyaw標本は別の出力先で監査する。

## 上下狙い打点を含む保存計画

[上下狙い診断](plan_aim_height_sweep.md)のmetadata version=3にも対応する。宣言した上下オフセットと `planned_aim_offset_world_m`、予測球中心から生成される `planned_aim_point_world_m` の一致を検査する。高さ別集計は変更していない予測球中心を使い、狙い位置の移動によって高さ帯の所属を変えない。
