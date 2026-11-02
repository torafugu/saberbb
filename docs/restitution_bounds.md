# 法線反発の低下だけで弱いフェア打球を作れるか

## 目的

文献値の自由剛体でも弱いフェア打球が少なかったため、接触を固定し、
反発係数eを0～0.5の範囲で下げたときの可能範囲を調べる。
この範囲で作れない球と、損失を増やせば作れる球を分ける診断であり、
振動モデルの同定や本体の反発係数変更ではない。

質量・重心・慣性には[文献木製バットの自由剛体](measured_bat_mass_preset.md)を使う。
接触位置・法線・接触点速度・投球速度を固定する。
e=0でも球に力積は加わる。「反発なし」は法線相対速度についての条件であり、
球が静止することや、必ず最小EVになることを意味しない。

## 解析式と境界

接近中の法線相対速度をu_n、逆有効質量をμ、球質量をm、単位法線をnとする。

```text
K = -u_n / (1 + m*μ) > 0
v_out(e) = v_in + (1+e)*K*n = A + B*e
A = v_in + K*n
B = K*n
```

保存された世界座標のXは横、Yは投手方向、Zは上。
フェア方向は`v_out_y > |v_out_x|`、つまりY-X>0かつY+X>0。
±45°の境界、水平速度0は含めない。試合の最終Foulや安打とは別の区分。
2つの一次不等式と閉区間[0,0.5]を交差させ、フェアとなるeの区間を求める。

EV²はeの凸二次式なので、頂点と区間端点から下限を計算できる。
EV<25 m/sとなる二次不等式の区間をさらに交差させる。
eを一定刻みで走査せず、狭い区間も解析式で求める。EV=25は弱打に含めない。

フェア区間の端が開いていて、その境界でEVが最も小さくなる場合がある。
その値は**下限（infimum）であり、フェアな球として達成された最小値ではない**。
`fair_min_attained=0`を確認し、境界のEVを実際のフェア打球として扱わない。
`fair_infimum_e`もその場合は極限を表す境界値である。
弱打が可能かは下限の丸め値ではなく、厳密な不等式の交差区間で判定する。

法線力積のみなので、球の世界座標での接線速度は変わらない。
`tangent_floor_ms = |v_in - (v_in·n)*n|`は、eをどう変えても消せない速度の下限。
バットに対する相対接線速度と取り違えない。
反発係数と摩擦・接線応答を同じ変更として扱わない。

## 実行

以前の観測更新診断の出力ディレクトリを指定する。必要な入力は次の3ファイル。

- observation_update_contacts.csv
- observation_update_metadata.json
- observation_update_coverage.csv

```sh
python3 tools/diagnostics/restitution_bounds_probe.py \
  --input-dir ../saberbb_observation_update \
  --output-dir ../saberbb_restitution_bounds
```

有効質量の集計CSVを入力するものではない。新しい試合・DB移行・接触探索は不要。
元ファイルは読み取り専用で、キー検査には一時SQLiteを使う。
既存出力への上書きは拒否する。途中で入力異常により停止した場合の部分出力は結果として使用せず、
入力を確認して別ディレクトリへ再実行する。

## 出力

| ファイル | 内容 |
|---|---|
| restitution_bounds_coverage.csv | 元試行・幾何接触・力積接触・弱いフェア方向が可能な試行の件数 |
| restitution_bounds_checks.csv | 元の一定質量結果の再現、文献値の反動、e=0/0.5の反発則・保存則・解析式の残差 |
| restitution_bounds_summary.csv | 条件別・打点帯別の件数、分母、EV下限・接線速度下限の平均 |
| restitution_bounds_diagnoses.csv | 条件別・打点帯別の可能/不可能の理由 |
| restitution_bounds_weak_hi.csv | 弱打区間の上限e帯×開閉、下限e・必要低下量の平均、基準フェア/強打の件数 |
| restitution_bounds_fixed.csv | 共通のe=0/0.1/0.2/0.3/0.4/0.5でのEV・方向・弱打/強打率・同一フェア集合の比較 |
| restitution_bounds_fixed_transitions.csv | 基準e=0.5→各eの方向×EV帯の件数 |
| restitution_bounds_contacts.csv | 1球×予測条件×Lの個別判定・eの区間・開閉フラグ・EV下限 |
| restitution_bounds_metadata.json | 入力ハッシュ、文献値、反発係数範囲、厳密境界の定義 |

最初は**coverage・checks・summary・diagnoses・weak_hi・fixed・fixed_transitionsの7 CSV**を添付する。
contactsは大きくなるため、個別追跡が必要な場合に対象を抽出する。
例として、次で「元の文献値e=0.5でもフェアだったが、減衰で初めて弱打を作れる球」を抽出できる。

```sql
-- CSVを任意のSQLiteテーブル restitution_bounds_contacts に読み込んだ後
SELECT game_id, count_seq, variant, fair_e_lo, fair_e_hi,
       weak_e_lo, weak_e_hi, weak_e_lo_open, weak_e_hi_open,
       baseline_ev_ms, e0_ev_ms, fair_ev_infimum_ms, fair_min_attained
FROM restitution_bounds_contacts
WHERE variant = 'observed_sigma_0.005_r0.9'
  AND baseline_fair = 1
  AND possible_only_after_reduction = 1;
```

CSVの開閉フラグはTrue/False。数値の区間端だけを見て、開端を使用可能なeへ含めない。
未接触・探索未解決・初期重複・非接近は診断未評価とし、EVや可能フラグは空欄。
0件という評価結果と混同しない。

## 理由と分母の読み方

理由は次の順に分類し、1試行を1分類へ入れる。

| diagnosis | 意味 |
|---|---|
| weak_fair_possible | 範囲内にEV<25かつフェアとなるeが存在する |
| no_fair_in_range | 範囲内にフェアとなるeが存在しない |
| tangent_floor_ge_threshold | フェア方向は存在するが、変わらない接線速度だけで25 m/s以上ある |
| normal_response_floor_ge_threshold | 接線速度は25未満だが、範囲全体の最小EVが25以上 |
| fair_constraint_blocks_weak | 範囲内でEV<25にはできるが、そのeではフェア方向にならない |
| not_evaluated:... | 接触・接近条件を満たさず未評価 |

不可能という判定は**この法線力積モデル・固定接触・e∈[0,0.5]の中での判定**。
実際の打撃で不可能という意味ではない。

summaryのbaselineは今回の**文献自由剛体・e=0.5**。従来の一定0.6 kgと混同しない。
`possible_only_after_reduction_contacts`は、e=0.5では弱いフェアでなく、下げると可能になる件数。
その中にはe=0.5ではファウル方向だった球もある。
元からフェアの球に限る場合は`baseline_fair_weak_possible_contacts`と
`weak_possible_pct_of_baseline_fair`を使う。既に弱いフェアだった球もこの分母・分子に含む。

`weak_possible_pct_of_impulse`の分母は力積接触数。
`fair_ev_infimum_ms`の平均はフェア区間が存在する接触だけで計算する。
各平均にはn列を添え、未評価を0で埋めない。
scope（all_swings / recorded_batting_only）、variant、stageは重複する見方なので合算しない。
coverageの試行数も複数予測条件・Lを含み、ユニークな球数ではない。

まず前回と同じ728430試行・514997幾何接触か、入力ハッシュと残差を確認する。
次にall_swings / observed_sigma_0.005_r0.9 / overallで理由の件数を確認し、
signed_axialの支点側と先端側、baseline_impactの衝突方向帯へ分ける。
e=0の結果だけで弱打の可能性を判断しない。

解析式は現行の摩擦なし衝突式をeについて整理したもので、損失曲線の文献フィットではない。
この診断の後、必要な反発低下の範囲が妥当か、あるいは接触点速度・法線・接線応答を調べるかを判断する。

## 必要な反発低下の大きさ

`weak_e_hi`は「弱いフェア方向となるeの区間の上限」。最大EVを表す値でも、
EVを最小にする`fair_infimum_e`でもない。上限が高いほど、小さな反発低下で弱打にできる。
`weak_e_hi_open=True`なら、その上限ちょうどでは弱打にできない。
下限も残し、eを下げすぎてファウル方向や弱打区間外へ出る場合を区別する。

weak_hiの帯は[0,0.1)、[0.1,0.2)、[0.2,0.3)、[0.3,0.4)、[0.4,0.5)、ちょうど0.5。
上限の開閉は別の行として数える。弱打不可能はnot_possible、未評価はnot_evaluatedへ分ける。
`avg_required_reduction_infimum = avg(0.5 - weak_e_hi)`は必要な**反発係数低下量の下限**。
上限が開いていれば、その低下量ちょうどでは足りない。これはエネルギー損失率ではない。
0.5→0.2を「エネルギーを60%失う」と解釈しない。

各行の`pct_of_weak_possible`は、同じscope/variant/stage/band_1/band_2で弱打可能な試行を分母とする。
不可能・未評価行では区間平均・割合を空欄とする。
元からフェアだった球だけを見る場合は`baseline_fair_contacts`と
`pct_of_baseline_fair_weak_possible`を使う。
`stratum_...`列の分母は各上限帯へ繰り返し記載されるので合計しない。
同じ既存のsigned_axial・baseline_impact帯で集計するため、軸方向打点と局所b/Rを混同せずに比較できる。
baseline_impactのband_2は基準の法線速度比。新しいeごとに帯を動かさない。

## 全接触へ共通のeを適用した場合

fixedのeは球ごとに選ばず、同じ集合の全接触へ共通に適用する。
解析式の弱打可能区間を求める方法は従来通りで、この6点の比較に置き換えるものではない。
狭い弱打区間が6点の間にあれば、弱打可能でも固定eでは弱打0件になり得る。

- `fair_contacts`と`foul_contacts`はそのeでの方向。水平速度0はundefined_directionへ分ける。
- `weak_pct_of_fair`と`strong_pct_of_fair`の分母はそのeのfair_contacts。EV<25を弱打、EV>=40を強打とする。
- `before_paired_fair_ev_ms`と`after_paired_fair_ev_ms`は、基準e=0.5と比較先eの**両方でフェア**となる同じ球の平均。
- `baseline_fair_to_weak_pct`は、元からフェアだった固定集合のうち、比較先で弱いフェアとなる割合。
- `strong_to_weak_pct`は、元からフェアかつEV>=40だった固定集合に対する弱いフェアへの変化率。

元から強打だった集合は、strong_to_weak_fair、strong_to_medium_fair（25<=EV<40）、
strong_to_strong_fair、strong_to_nonfairへ分かれる。最後はファウルと未定義方向を含む。
方向を跨ぐ球の詳細はfixed_transitionsを使う。EV帯は全方向で分けているため、
EV<25でもafter_directionがfoul_directionなら弱いフェアとして数えない。
未評価はnot_evaluatedとし、EV平均は空欄。0 m/sを埋めて平均を下げない。

まず同じscope/variant/stageでe=0.5の件数とEVが前回を再現するかを確認し、
次にe=0.4/0.3/0.2/0.1でどれだけ弱打が増え、強打が減り、方向が変わるかを見る。
e=0は極限の比較として追加し、本体への採用候補と決めない。
eごとの行は同じ試行を繰り返した比較なので合算しない。
個別contactsと従来の解析集計は維持し、checksへ固定eの保存則残差を追加する。
