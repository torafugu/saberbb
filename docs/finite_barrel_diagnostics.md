# 有限バレル半径・球の回転・法線接線連成の診断

## 目的と段階

打点依存の有効反発を維持し、中心線上へ力積を与えた従来の回転モデルを、
有限半径のバット表面での力積へ拡張する。
球のEV/HLA/VLA・回転、バットの並進・横回転・軸回転を同じ力積から求める。
本体への採用前の衝突診断であり、弱打やrpmを目標値へ合わせる処理ではない。

## 実行

従来と同じ3ファイルを入力する。

- observation_update_contacts.csv
- observation_update_metadata.json
- observation_update_coverage.csv

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update \
  --output-dir ../saberbb_finite_barrel
```

試合・接触探索の再実行やDB移行は不要。入力を変更せず、出力先の上書きを拒否する。
失敗時の部分出力は使わず、原因を確認して新しい出力先へ再実行する。

デフォルトの軸回り慣性は0.00015/0.00030 kg m²の**仮定値**。
Nathanの横慣性と混ぜた感度比較であり、同じ文献バットについて実測された軸慣性ではない。
入射回転は前と同じく0 rad/sの仮定、接地までの高さは0.9 mの仮定。
必要なら個別に変更する。

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update \
  --output-dir ../saberbb_finite_barrel_topspin \
  --axial-inertias 0.00015 0.00030 \
  --ball-spin-rad-s -200 0 0 --height-m 0.9
```

回転は右手系のワールドX/Y/Z成分で単位rad/s。rpmではない。
高さは球中心の初回地面接触までの鉛直移動量。

## 保存幾何と表面接触

球半径r=0.036 m、球慣性I_ball=0.4*m*r²を前回から継承する。
バット半径Rは `保存された有効接触半径 - r` とする。
有効半径0.07 mの場合R=0.034 m。実物バットの測定形状への置き換えではない。
これにより、球中心とバット中心線の距離R+rは保存した接触幾何と整合する。

側面接触だけを扱い、端部・有効e曲線範囲外は未評価。
探索時刻・打点・軸・法線を変更しない。
保存されたpoint_velocityは中心線上の点の速度であり、表面速度へ次の補正を行う。

```text
a = バット軸、n = バットから球への法線、t = a×n
s = 保存打点、c_cm = 保存基準点に対する重心位置、d = s-c_cm
r_bat = d*a + R*n
r_ball = -r*n
v_surface = v_centerline + omega_bat × (R*n)
v_cm = v_centerline - omega_bat × (d*a)
u = v_ball + omega_ball × r_ball - v_surface
```

バットは軸対称の慣性テンソルを持つ剛体とする。
横慣性は前のNathan仕様、軸慣性は上記の感度仮定。
球とバットの両方の接触点を共通の空間点に置けるので、系全体の角運動量も検査できる。

## 法線・接線の連成

接触点の相対速度変化は `Δu=K*J`。
基底(n,a,t)で逆有効質量行列は次となる。

```text
K_nn = 1/m + 1/M + d²/I_perp
K_aa = 1/m + 1/M + R²/I_perp + r²/I_ball
K_tt = 1/m + 1/M + d²/I_perp + R²/I_axis + r²/I_ball
K_na = K_an = -d*R/I_perp
その他の非対角成分 = 0
```

R²/I_axisがバットの軸回転による反動。
K_naは法線と軸方向接線の連成で、接線力積を変えると法線力積も変わる。
従来の法線力積を固定したまま接線力積だけ追加する方式にはしない。

目標は法線相対速度`u'_n=-e_n*u_n`、接線相対速度`u'_a,t=-e_t*u_a,t`。
法線条件でJnを消去して接線2成分を解く。
接線目標が摩擦円錐 `|J_tangent|<=mu*Jn` を超える場合は、
目標接線力積へ向かう直線上で縮小し、法線条件からJnを再計算する。

**これは終端力積の近似**であり、接触中の変形・滑り・固着を時間積分するものではない。
最大散逸摩擦問題の厳密解でもなく、滑り方向が時間とともに変わる一般問題を解いたとは主張しない。
e_nとe_tを連成軸へ独立に設定すると、条件によってはエネルギーが増えることがある。
その場合は `not_evaluated:energy_increasing_target` として採用せず、未評価件数に残す。
引張りの法線力積が必要になる目標も `not_evaluated:tensile_normal_target` とする。
EVやrpmを後処理で縮小して、保存則が通ったことにしない。

## エネルギー・運動量

一つのJで球とバットの並進・回転を更新する。

```text
v_ball' = v_ball + J/m
omega_ball' = omega_ball + (r_ball×J)/I_ball
v_cm' = v_cm - J/M
omega_bat' = omega_bat - I_bat^-1*(r_bat×J)
```

総運動エネルギーの変化は `u·J + 0.5*J·K*J` であり、その負値を有効全損失として記録する。
球の並進/回転、バットの運動、全損失から収支を確認する。
連成があるため、全損失を独立の法線損失・接線損失へ機械的に分割しない。
位置依存e_effに含めた球の変形・振動を、身体損失や振動損失としてさらに差し引かない。

法線反発、達成可能な場合の接線反発、摩擦円錐、行列応答、
球・バット各々の角力積、系の全角運動量、並進運動量、エネルギー収支を検査する。

## 比較する8モデル（デフォルト）

| モデル | 用途 |
|---|---|
| normal_profile | 前の打点依存法線応答。球の入射回転は変えずに持ち越す |
| centerline_et0 / centerline_et0.4 | 前の中心線回転モデル、mu=0.2 |
| finite_mu0 | 有限半径・摩擦なし。normal_profileとの一致を確認 |
| finite_I0.00015_et0 / et0.4 | 有限半径・低い軸慣性、mu=0.2 |
| finite_I0.0003_et0 / et0.4 | 有限半径・高い軸慣性、mu=0.2 |

全モデルで打点依存eと適用可能な元接触集合を揃える。
mu=0.2とet=0/0.4は前回からの感度条件で、実データへの校正値ではない。
保存値へ入射球回転がない点も変わらない。

モデルの根拠となる有限半径・軸慣性の役割は
[Kensrud, Nathan, Smith (2017)](https://baseball.physics.illinois.edu/ObliqueCollisionsWSU-AJP.pdf)
のEq.(7)とTable IIを参照。論文の散乱面モデルをそのまま再現したものではない。
同表にある別の木製バットの軸慣性や反発係数を、Nathanバットの実測値として流用しない。

## 出力・確認順

| 出力 | 内容 |
|---|---|
| finite_barrel_coverage.csv | 元試行・接触、文献曲線の適用可能件数、モデル数 |
| finite_barrel_checks.csv | 元接触再現、摩擦なし/半径ゼロの理論的対応、保存則などの残差 |
| finite_barrel_status.csv | variant×モデル×評価状態×接線応答区分の件数 |
| finite_barrel_summary.csv | 打点別のEV/VLA、回転ベクトル、エネルギー、地面応答など |
| finite_barrel_pairs.csv | 中心線→有限半径の同一成功接触上の差、フェア方向の遷移 |
| finite_barrel_examples.csv | 先頭最大500モデル行の個別値（代表標本ではない） |
| finite_barrel_metadata.json | 入力ハッシュ、半径・慣性・回転・近似法などの仮定 |

最初は **coverage・checks・status・summary・pairsの5 CSV** を添付する。

1. checksとstatusを確認し、エネルギー増加などで未評価となる条件の有無を見る。
   coverageのeligibleは文献曲線上の元接触数で、各連成モデルの成功件数ではない。
2. `all_swings / observed_sigma_0.005_r0.9 / overall` のpairsで、
   normal_profile→finite_mu0が一致していることを確認する。
3. centerline→finiteを同じetで比較し、EV低下と回転変化、軸慣性への感度を見る。
4. `stage=source_z` で根元寄り・基準点・先端寄りを分ける。
5. 基準法線モデルがフェア方向の集合に固定するなら、
   `stage=source_z_reference_direction / band_2=fair_direction` を選ぶ。
   比較後に非フェアへ変わった接触もこの集合内に残し、pairsの遷移件数で確認する。
6. 回転数の大きさだけでなく、ワールド回転ベクトルとバットの軸回転も見る。
7. summaryの各n_列、pairsのpaired_contactsと各n_列が平均の分母。
   未計算項目は空欄であり、0 Jや0 rpmではない。

半径ゼロで従来の中心線モデルへ戻ることは自動テストで検証する。
CLIのchecksには実行した有限半径・摩擦なし対照の一致を記録する。

地面応答は前と同じ重力のみ・初回接地と反跳の近似。
抗力・Magnus・風・塀・野手・転がりはなく、正VLAの遠距離飛行は実際の試合と同一視しない。
低く遅いゴロの目標割合を設けず、衝突の整合性と実測校正を分けて進める。

単一実験仕様での文献式との比較と、元ゲームDBから各球の回転を読み込む手順は
[衝突モデルの文献比較と投球回転](collision_validation.md)を参照。
非ゼロ回転の実行ではfinite_barrel_spin_pairs.csvにゼロ回転との同一球比較を出力する。
