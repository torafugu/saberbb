# スイング前の予測だけで上下の狙いを選ぶ診断戦略

打者の「何を狙うか」と「実際にどんな打球になったか」を分ける第一段階。
Rust本体から独立したPython診断で、予測投球高と明示した打球意図からバット基準点の上下オフセットを決める。
既存の同一投球×狙い別キャッシュから選んだ計画の結果を取り出し、戦略間のトレードオフを比較する。
Statcastへの構成比フィット、速度の混合、最良戦略の自動選択は行わない。

## 最初の仮説

既定設定は `tools/diagnostics/pre_swing_aim_policies.json`。
境界は予測球中心のworld zで、0.6 mと0.9 m。境界値は上側の区分に含む。
負のオフセットは予測球中心より下をバット基準点で狙う。

| 戦略 | 意図 | 予測高 <0.6 m | 0.6≤予測高<0.9 m | 予測高≥0.9 m |
| --- | --- | --- | --- | --- |
| contact_center | 接触重視の仮説・中心狙い対照 | 0 mm | 0 mm | 0 mm |
| fixed_under15 | 一律の浅い下側狙い対照 | −15 mm | −15 mm | −15 mm |
| fixed_under30 | 一律の深い下側狙い対照 | −30 mm | −30 mm | −30 mm |
| line_drive_height | ライナー志向・高い予測ほど下側狙いを減らす仮説 | −30 mm | −15 mm | 0 mm |
| lift_height | 浮かせる志向・高い予測でのみ下側狙いを減らす仮説 | −30 mm | −30 mm | −15 mm |

意図の名前は達成結果を保証しない。中心狙いが接触率最大であることも仮定しない。
高さの境界とオフセットは診断用の仮説であり、実測から推定した値でも、最適化した値でもない。
高さは選手別ストライクゾーンで正規化していない。低い予測に強めの下側狙いを与える仮説が妥当かも、今回評価する対象である。
VLAからオフセットを逆算する衝突モデルはまだ導入していない。

## 情報の境界

`pre_swing_aim.py`は接触計算モジュールをimportしない。
`Forecast(height_m)`と固定した`AimPolicy`から上下の狙いを返す。
打球意図は外部から固定して渡し、接触成功・法線・実際の投球高・EV/VLAから意図を選び直さない。
球種・カウント・打者能力・予測の不確実性等は、この初版の入力に加えていない。

評価側は、傾斜・yaw・速度の各組み合わせ内で予測だけから全戦略の選択を確定し、その後に選択したキャッシュの接触・打球結果を集計する。
元キャッシュに全投球・全条件があること、予測が条件間で共通であること、宣言した狙いと保存した計画が一致することを既存の監査で検証する。
ルールの必要な狙いが一つでもなければ、未使用の高さ区分でもエラーにする。近い狙いへの置換や結果を見た代替選択は行わない。

速度は今回の戦略で選ばない。たとえば速度比0.6/0.8/0.9/1.0が保存されていれば、それぞれで同じ戦略群を評価する。
各速度・傾斜・yaw・stratumは別の比較であり、行を足して混合戦略と解釈しない。

## 実行

既存のseed2キャッシュが傾斜15度・yaw 0度・狙い−30/−15/0 mmを含む場合：

```sh
python3 tools/diagnostics/pre_swing_aim_probe.py \
  --input-dir ../saberbb_plan_midpoint_holdout_seed2 \
  --output-dir ../saberbb_pre_swing_aim_seed2
```

DB、Statcast CSV、再探索、衝突再計算は不要。キャッシュは`plan_tilt_states.jsonl`等を含む元の計算出力フォルダを指定する。
混合比較や高角度監査の集計出力フォルダではない。
出力先には新しいフォルダを指定する。独自の仮説は同じ形式のJSONを`--policies-json PATH`で指定できる。
ルールはmetadataに内容とSHA256を保存する。

seed2は既に結果を見た標本なので、この比較は探索的診断とする。
別標本で確認するときはルールを先に固定し、既存の標本除外オプションで未使用投球を確保する。たとえば：

```sh
python3 tools/diagnostics/plan_speed_sweep.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_aim_policy_holdout_seed3 \
  --sample-size 1000 --sample-seed 3 \
  --exclude-sample ../saberbb_plan_speed_aim_sample/plan_tilt_sample.csv \
  --exclude-sample ../saberbb_plan_speed_aim_holdout_seed1/plan_tilt_sample.csv \
  --exclude-sample ../saberbb_plan_midpoint_holdout_seed2/plan_tilt_sample.csv \
  --tilts-deg 15 --yaws-deg 0 \
  --speed-ratios 0.6 0.8 0.9 1 --aim-z-mm -30 -15 0

python3 tools/diagnostics/pre_swing_aim_probe.py \
  --input-dir ../saberbb_aim_policy_holdout_seed3 \
  --output-dir ../saberbb_aim_policy_holdout_results_seed3
```

除外パスは実際に使用済みの標本に合わせる。他にも使用済みの投球があれば、そのsampleを追加して除外する。
新しいキャッシュの作成だけは元の観測更新出力と読み取り専用DBが必要。同じ選手・試合由来の依存は残る。

## 出力の読み方

最初に次の5ファイルを共有する。

- `pre_swing_aim_summary.csv`：戦略・速度等ごとの全体行と予測高別行。
- `pre_swing_aim_pairs.csv`：同じ投球での全戦略の組み合わせ比較。方向はafter−before。
- `pre_swing_aim_selections.csv`：予測高別にどの狙いを何投球選んだか。
- `pre_swing_aim_checks.csv`：元キャッシュの予測・計画・集計整合性。
- `pre_swing_aim_metadata.json`：固定した戦略と元標本・制約。

`pre_swing_aim_decisions.jsonl`には全投球×戦略×速度等の選択を保存する。予測高、戦略、狙い、選択したscenarioを含み、接触・打球結果は含まない。詳細確認時に利用する。

summaryの主な列：

| 列 | 分子・分母／意味 |
| --- | --- |
| geometry_probability_per_evaluable_trial | 幾何接触件数÷時計が成立した投球数 |
| successful_probability_per_evaluable_trial | 力積が正の接触件数÷時計が成立した投球数 |
| fair_probability_per_evaluable_trial | フェア方向打球件数÷時計が成立した投球数 |
| downward_probability_per_evaluable_trial | フェア方向かつVLA<0度の件数÷時計が成立した投球数 |
| high_vla_probability_per_evaluable_trial | フェア方向かつVLA≥50度の件数÷時計が成立した投球数 |
| line_window_probability_per_evaluable_trial | フェア方向かつ10≤VLA<30度の件数÷時計が成立した投球数 |
| downward/high_vla/line_window_fraction_among_fair | 上記の各件数÷その戦略で得たフェア方向打球数 |
| mean_fair_ev_ms / mean_fair_vla_deg | その戦略で得たフェア方向打球内の平均 |
| clock_invalid / unresolved / overlap_at_start | 時計不成立／探索未収束／探索開始時近接。空振りとは分ける |
| unsupported_cap / out_of_curve / pivot_below_ground | 端部・曲線範囲外の評価除外／支点が地面下になった投球数 |

line_windowはライナー志向を評価するための便宜上の角度帯で、実際のライナー分類とは異なる。
フェア方向はHLAの絶対値が45度未満の方向分類で、最終フェア判定ではない。
時計不成立は分母から除外する。未収束・開始時近接は評価対象投球数に残すが、非接触確定とは扱わない。
フェアがゼロの条件付き割合や共通フェアがゼロの対応指標は空欄になる。

pairsでは接触・フェアのgained/lostと、各イベントの全評価可能投球あたりの差を確認する。
`common_fair_*`は両方の戦略でフェア方向となった同じ投球だけでの変化。
条件付き高角度率が下がっても、高角度になりやすい投球で接触を失った結果かもしれないため、全投球あたりの接触率・フェア率と併せて読む。
同じ高さ区分の全戦略が同じ狙いを選べば、その区分では同じ結果となる。

## 検証

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_pre_swing_aim.py'
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
```

境界値、事後情報を渡せない入力型、事後結果を変えても不変な選択、固定狙い対照との一致、
時計不成立と探索未収束の区別、分母・高さ区分の集計、共通フェアと入れ替わり、
不足条件の拒否、DB・探索・衝突計算を呼ばないキャッシュ処理を確認する。

次の段階は、このトレードオフを読んで戦略の意図と狙い量の関係を検討すること。
カウント・打者特性・予測誤差に応じた意図選択やRust本体への組み込み、実際の戦略構成比の推定は別に検討する。

## seed2を見て固定した次の仮説

seed2の1,000投球（評価可能994投球）では、通常速度の旧ライナー志向のライナー帯発生率は13.88%で、
中心狙い14.99%を下回った。特に予測高0.6 m未満で、中心→−30 mmにするとフェア方向率が
61.90%→47.62%、ライナー帯発生率が17.37%→11.20%となった。
中間・高めでは−15 mmが次の検証候補になったため、次の1条件を追加して固定する。

| 戦略 | 予測高 <0.6 m | 0.6≤予測高<0.9 m | 予測高≥0.9 m |
| --- | --- | --- | --- |
| line_drive_low_center | 0 mm | −15 mm | −15 mm |

設定は `tools/diagnostics/pre_swing_aim_seed2_frozen.json`。
旧5戦略を同じ名前・定義で残し、新条件を加えた6戦略で比較する。
従来の既定ファイルは変更しない。新しい検証では必ず`--policies-json`でこの固定ファイルを指定する。
選択器・高さ境界・物理係数・速度条件は従来と共通で、狙いの構成比は調整しない。

これは**seed2の結果を見て作った仮説**であり、seed2での再集計は独立した検証ではない。
次の手順で、seed0・seed1・seed2の投球をすべて除外したseed3を作成する。
以前にほかの標本も見ている場合は、対応するsample CSVも`--exclude-sample`に追加する。

```sh
python3 tools/diagnostics/plan_speed_sweep.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_aim_policy_holdout_seed3 \
  --sample-size 1000 --sample-seed 3 \
  --exclude-sample ../saberbb_plan_speed_aim_sample/plan_tilt_sample.csv \
  --exclude-sample ../saberbb_plan_speed_aim_holdout_seed1/plan_tilt_sample.csv \
  --exclude-sample ../saberbb_plan_midpoint_holdout_seed2/plan_tilt_sample.csv \
  --tilts-deg 15 --yaws-deg 0 \
  --speed-ratios 0.6 0.8 0.9 1 --aim-z-mm -30 -15 0

python3 tools/diagnostics/pre_swing_aim_probe.py \
  --input-dir ../saberbb_aim_policy_holdout_seed3 \
  --output-dir ../saberbb_aim_policy_frozen_results_seed3 \
  --policies-json tools/diagnostics/pre_swing_aim_seed2_frozen.json
```

seed3キャッシュが既にある場合は、それが今回の仮説を固定する前に未確認だった標本か、
metadataの除外元に使用済みの全標本が入っているかを確認する。単にseedを変えただけでは投球重複を除外できない。
上の出力先が既存なら上書きせず、別の新しい名前を使う。キャッシュ作成時のみDBが必要。

評価する主指標は、速度比1.0における新条件と中心狙いの**評価可能投球あたりのライナー帯発生率の差**。
接触率・フェア方向率・高角度打球発生率も併記し、ライナー帯が増えても接触損失などの代償を確認する。
固定−15 mmとの比較は「低めを中心狙いに戻す効果」を、旧ライナー志向との比較は変更前後を調べる補助比較とする。
速度比0.6/0.8/0.9と予測高別の結果は感度確認として別々に読む。
条件付き割合だけで判断せず、pairsのgained/lostとcommon_fairも確認する。

新しい許容幅や合否判定は設定していない。差の符号・大きさと代償を記録する段階であり、
改善方向の差だけで有意差や最適性を主張しない。seed3の結果を見てこの固定ファイルを調整しない。
変更が必要なら別名の仮説と次の未使用標本を用意する。同じ選手・試合由来の依存は引き続き残る。

`saberbb_aim_policy_frozen_results_seed3`から、前述のsummary・pairs・selections・checks・metadataの5ファイルを共有する。
共有されたseed3の集計では、1,000投球中995投球が評価可能だった。通常速度の新条件対中心狙いで、
ライナー帯は120→150球（投球あたり+3.02ポイント）、フェア方向は634→635球となった。
これは点推定であり、次のブートストラップでは同じ固定設定・元標本を使って区間を求める。

## 固定戦略の不確実性評価

seed3で得た点推定を変更せず、投球単位の対応を保った区間推定を行う手順は
[固定戦略の対応ブートストラップ](pre_swing_aim_bootstrap.md)を参照。
