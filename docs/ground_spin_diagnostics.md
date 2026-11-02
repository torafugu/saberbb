# EV×VLA・地面衝突後と回転を一緒に調べる

## 目的と判定の分離

EV<25 m/sだけでは、高EVで地面へ向かい、衝突後に水平速度が小さくなる打球を拾えない。
ただし「下向き」「地面衝突後に遅い」「低く転がる」は別の性質。
高EVの急角度打球が高く跳ねることもあり、水平速度だけでボテボテと断定しない。

本診断は、打ち出しのEV×VLA、初回接地距離・時間、接地前後の速度、回転、
反跳の高さと次の接地までの距離を分けて出力する。
`low_ev_contacts` は従来のEV<25、`slow_after_ground_contacts` は接地直後の水平速度が閾値未満。
`high_ev_downward_slow_contacts` はEV>=40、VLA<0、接地直後の水平速度が閾値未満の共通集合。
これらは重複し得る診断集合であり、試合の安打・アウト・弱打種別ではない。
各項目の `fair_` 付き件数は、打ち出しがフェア方向の集合に限定する。

## 実行

従来と同じ観測更新出力の3ファイルを入力にする。

- observation_update_contacts.csv
- observation_update_metadata.json
- observation_update_coverage.csv

```sh
python3 tools/diagnostics/ground_spin_probe.py \
  --input-dir ../saberbb_observation_update \
  --output-dir ../saberbb_ground_spin
```

新しい試合、DB移行、接触再探索は不要。既存出力先の上書きは拒否する。
途中で失敗した出力は結果として使わず、原因を確認して別の出力先へ再実行する。

接触高さ・球の入射回転ベクトルは元CSVに保存されていない。
デフォルトは高さ0.6/0.9/1.2 m、球の入射回転0 rad/sの**仮定条件**である。
高さは初回地面接触までの球中心の鉛直移動量（点質量の飛行規約）。
実際の球中心高度を与える場合は、半径を差し引いた値にする。
地面の反発0.45・摩擦0.25は本体と同じ数値を初期仮定にするが、接地アルゴリズムは異なる。
閾値25 m/sは比較用で、ボテボテの実測基準ではない。

```sh
python3 tools/diagnostics/ground_spin_probe.py \
  --input-dir ../saberbb_observation_update \
  --output-dir ../saberbb_ground_spin_topspin \
  --heights-m 0.9 \
  --ball-spin-rad-s -200 0 0 \
  --ground-e 0.45 --ground-mu 0.25 --slow-ms 25
```

回転は右手系のワールドX/Y/Z成分。rpmではない。200 rad/s ≒ 1910 rpm。
+Yへ進む球では-X回転がトップスピンとなる。
他の方向へ飛ぶ球に同じワールド回転を与えると相対的な回転の意味も変わる。
0、±X、必要なら±Zを別出力先で比較する。

## 比較モデル

以前の5法線モデル（一定質量、Kensrud剛体、Nathan剛体、Fig.12実線/破線）を保ち、
Fig.12実線を共通の法線条件とする接線モデルを2つ追加する。

| モデル | 接線応答 |
|---|---|
| nathan_fig12_full_speed | 接線力積0、球の入射回転をそのまま持ち越す |
| nathan_full_mu0.2_et0 | 摩擦上限内で接触点の滑りを止める |
| nathan_full_mu0.2_et0.4 | 摩擦上限内で接線相対速度を反転し、接線弾性の影響を見る |

mu=0.2、et=0/0.4は感度比較用の仮定で、文献から採用した校正値ではない。
法線有効eの文献曲線へ、同じ振動損失を再度乗算しない。
端部・文献曲線範囲外・非接近は接線モデルの未評価として残す。
未評価をEV=0にせず、比較可能な共通接触だけをpairsの分母にする。

## 衝突式とモデルの限界

球は質量m=0.145 kg、半径r=0.036 m、慣性I_ball=0.4*m*r²の一様球。
この半径と慣性係数は診断の仮定で、保存された有効接触半径0.07 mとは区別する。
バットは以前のNathan質量・横慣性を使う。
法線n、バット軸a、t=a×n、重心からの軸方向距離dとし、
力積を保存済みのバット中心線上の点へ作用させる簡略モデル。
**有限バレル半径・バット軸回りのねじり・法線接線間の連成は含めない。**
そのため法線応答は以前の剛体式と一致する。
探索形状や接触法線を変更せずに接線応答だけを比較するための段階的な試作で、
実際の有限半径バットを完全に表すものではない。

球の接触腕r_ball=-r*nとして、接触点相対速度を求める。

```text
u = v_ball + omega_ball × r_ball - v_bat_point
K_n = 1/m + 1/M + d²/I_bat
K_a = 1/m + 1/M + r²/I_ball
K_t = K_n + r²/I_ball
J_n = -(1+e_n)*u_n/K_n
J_a,target = -(1+e_t)*u_a/K_a
J_t,target = -(1+e_t)*u_t/K_t
```

接線力積の大きさがmu*J_nを超える場合は、2成分を同比率で縮小する。
方向依存の有効質量に対する、最大散逸摩擦問題の厳密解ではない。
滑り継続/接線反発目標への到達を区別する終端力積の近似であり、
接触中の微小変形・滑りと固着の時間変化を積分するモデルではない。

```text
J = J_n*n + J_a*a + J_t*t
v_ball' = v_ball + J/m
omega_ball' = omega_ball + (r_ball × J)/I_ball
v_bat_cm' = v_bat_cm - J/M
omega_bat' = omega_bat - (d*a × J)/I_bat
```

EV/HLA/VLAはこの1本のv_ball'から求め、回転も同じJから求める。
球の速度から任意にエネルギーを差し引いてrpmを後付けする方式ではない。
球の並進と回転に加え、バットの並進と回転も含めた前後運動エネルギーを確認する。
法線損失は0.5*(1-e_n²)*u_n²/K_n、接線損失は
`-Σ(u_i*J_i + 0.5*K_i*J_i²)`。損失の非負性とエネルギー残差を検査する。
球の回転エネルギーが増えても、バットからの仕事も変化し得るため、
球の並進エネルギー減少分と必ず1対1対応するわけではない。

Kensrud / Nathan / Smith (2017),
[Oblique collisions of baseballs and softballs with a bat](https://baseball.physics.illinois.edu/ObliqueCollisionsWSU-AJP.pdf)
は接線弾性と回転増大を扱う。本試作の検討根拠であり、論文の完全な再実装ではない。
次段階ではバレル半径・横慣性と軸慣性を含む有効質量行列、連成した法線/接線応答、
実際の入射回転・接触位置の保存、実験の角度/速度/回転との比較が必要。

## 地面と飛行

初回接地は重力だけの放物運動で解析的に求める。地面衝突では球の接触点速度に対し、
法線反発と摩擦上限付き接線力積を計算して並進・回転を更新する。
その後の反跳頂点と次回接地までの時間/移動距離を求める。
地面衝突の並進＋回転エネルギー収支と、飛行中の位置エネルギーも検査する。
トップスピンから水平並進へエネルギーが戻る場合もある。

空気抵抗・マグヌス・風・塀・野手・連続バウンド・転がりは未実装。
高く遠く飛ぶ球も、この無限平面モデルではいずれ接地する。
特に正VLAの遠距離飛行は本体の打球軌道や試合結果と同一視しない。
まず負VLA帯の初回地面応答を確認し、将来は同じ回転ベクトルを本体の飛行計算へ渡す。

## 出力と読み方

| ファイル | 内容 |
|---|---|
| ground_spin_coverage.csv | 元試行数、適用可能/範囲外/端部等、モデル数・高さ条件数 |
| ground_spin_checks.csv | 元接触再現と、衝突・回転・地面・飛行の保存則残差 |
| ground_spin_status.csv | variant×モデル×接触可否、未評価も含む件数 |
| ground_spin_summary.csv | overall、EV×VLA、接地後水平速度×反跳高さの集計 |
| ground_spin_pairs.csv | 同一接触に対する法線のみ→接線ありの変化 |
| ground_spin_examples.csv | 高EV/下向き/接地後低水平速度の先頭最大500例、エネルギーを含む |
| ground_spin_metadata.json | 入力ハッシュ・全仮定・文献・条件 |

最初は **coverage・checks・status・summary・pairsの5 CSV** を添付する。
必要ならexamplesも追加し、metadataは条件追跡のため保存しておく。
examplesは先頭順の事例であり、ランダムな代表標本ではない。

summaryは `all_swings / observed_sigma_0.005_r0.9 / height_m=0.9` を起点にする。

1. checksの残差が1e-7以下、statusの適用/未評価件数が妥当か確認する。
2. `stage=ev_vla` は `band_1` がEV帯、`band_2` がVLA帯。この行で、EV>=40かつ負VLAのフェア方向に、接地後の低水平速度が何件あるか見る。
3. `stage=ground_response` は列名を共用しており、`band_1` が**接地後水平速度帯**、
   `band_2` が**反跳高さ帯**を意味する。低いバウンドと高い跳ね上がりを分ける。
4. `stage=overall` の `fair_low_ev_contacts`、`fair_slow_after_ground_contacts`、
   `fair_high_ev_downward_slow_contacts` を比較する。率はそれぞれ `fair_contacts` を分母にする。
5. pairsでは共通の `paired_contacts` 上の平均EV/VLA/rpm/接地後水平速度/反跳高さを見る。
   フェア→非フェアの件数も別欄にある。pairsの平均・速度遷移は両方フェアだけには限定しない。
   pairsの平均にも `n_` 列があり、未定義の方向角は各平均の分母から除く。
6. summaryの `n_` 列は各平均の分母。法線対照に未計算のエネルギー欄は空欄で、0ではない。

scopes、モデル、高さ、variant、集計stageは同じ入力の別の見方なので合算しない。
接地後水平速度が閾値未満というだけで、従来の弱打率へ直接加算しない。
