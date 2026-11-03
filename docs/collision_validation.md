# 衝突モデル：文献条件の比較と投球回転の入力

## 何を確認し、何をまだ確認できていないか

保存則と極限ケースの一致だけで、実際の打球速度や回転数へ適合したとは判断しない。
今回は、次を分けて進める。

1. 一つの実験に記載されたバット・球の仕様へ統一した比較。
2. 文献の平面衝突式と、有限半径3Dモデルの応答の比較。
3. 公開図の実測回帰傾向との参考比較。
4. ゲームの各投球の保存回転値を入力した、入射回転ゼロとの同一球比較。

**独立した実測検証の合格とはしない。** 文献の木製バット用接線反発係数は、
観測された回転から推定された値であり、その値を入力して回転が似ることだけでは独立検証にならない。
また、公開PDFには木製バットだけの個別打球数値列や装置の全軌道は記載されていない。
探索した公開資料から今回それらは取得できておらず、図の点を架空の精密データへ置き換えない。

## 文献の単一プリセット

出典：[Kensrud, Nathan, Smith (2017), Oblique collisions of baseballs and softballs with a bat](https://baseball.physics.illinois.edu/ObliqueCollisionsWSU-AJP.pdf)
（DOI: 10.1119/1.4982793）。Table I/II、Eq.(4),(7)〜(9)、Fig.3/4を使う。

| 項目 | 同じ実験の値 |
|---|---|
| 木製バット質量 | 29.41 oz |
| 横慣性 I0 | 2370 oz in² |
| 軸慣性 Iz | 9.1 oz in² |
| 重心から打点まで b | 6.8 in |
| 打点半径 R | 1.16 in |
| 球質量・半径 | 5.04 oz・1.42 in |
| 球慣性 | I=0.4*m*r²（文献と同じ一様球近似） |
| 木製バットの接線反発 ex | 0.464、標準誤差0.014（実験からの推定値） |
| 法線反発 ey | 0.5（Sec.Vで用いた仮定。木製バットの実測法線反発ではない） |

Nathanの質量・横慣性・振動曲線や、前の仮定軸慣性は混ぜない。
この比較では球の質量・半径も一致させる必要があるため、有限半径ソルバーへ球のパラメータ指定を追加した。
既存のゲーム診断のデフォルト球パラメータは維持する。

## 文献条件の実行

```sh
python3 tools/diagnostics/literature_collision_probe.py \
  --output-dir ../saberbb_literature_collision
```

ゲームDBや観測更新CSVは不要。既存出力先の上書きを拒否する。
静止・無回転の球へ、28 m/s・77 mph・39 m/sの局所バット速度、入射角0〜30度を与える。
接線反発0/0.4/0.464、摩擦0.2/0.5、初期バット角速度0/40 rad/sで計576条件。
角速度は装置の実測値ではなく、未報告の運動への感度を見る仮定。
バットの実際の回転支点・拘束条件を完全再現したとは扱わない。

文献の独立した平面式をreferenceとして併記する。

```text
ry = m*(1/M + b²/I0)
rx = m*alpha/(1+alpha)*(1/M + b²/I0 + R²/Iz)
v_ball,n = (1+ey)/(1+ry) * v_bat*cos(psi)
v_ball,t = -(1+ex)*alpha/((1+alpha)*(1+rx)) * v_bat*sin(psi)
omega_ball = -v_ball,t/(alpha*r)
```

実装は軸X、撮影・散乱面YZとする。平面式と比べる項目は、
**YZ面へ射影した速度・角度と、X軸回転の符号付き成分**。
3D速度の大きさや全回転数を、撮影面の観測量とそのまま比較しない。
3Dの全速度・全回転、面外速度・面外回転も別列で出す。

速度・回転は77 mph（34.42208 m/s）へ正規化する。
PDF本文の倍率表記は逆比に読める箇所があるため、本文の「速度に比例する」という性質に基づき、
`77 mph / 入力バット速度` を掛ける解釈を採用してmetadataに残す。

Fig.3の179 rpm/度、Fig.4の−0.65 mph/度は**複数バットをまとめた回帰線**。
木製バットだけの真値や目標値にはしない。
今回の入射角の等間隔グリッドは実験の標本分布とも異なるため、OLS傾きの差を合否判定に使わない。
特に、入射角psiと打ち出し角thetaは別で、回帰は計算されたthetaに対して行う。

| 出力 | 内容 |
|---|---|
| literature_collision_points.csv | 全576条件、平面式・3D射影応答・面外成分の比較 |
| literature_collision_summary.csv | 条件別の回帰傾き、成功/未評価件数、実測検証未確立の明示 |
| literature_collision_checks.csv | 保存則、平面式の角運動量関係などの残差 |
| literature_collision_metadata.json | 単一仕様、係数・回帰線の出典と適用上の制約 |

実行確認では576条件すべてが成功した。
77 mph、mu=0.5、仮定角速度0 rad/sでの3D射影応答の回転傾きは、
ex=0で約106.08、ex=0.4で約165.26、ex=0.464で約175.98 rpm/度だった。
これらは実装と文献傾向を調べる診断値で、新しい係数の校正結果ではない。

## 保存された各投球の回転を使う

観測更新CSVには球の回転がないが、元ゲームDBのplayer_game_pitchingには
spin_rate（rpm）、spin_angle（度）、spin_efficiencyがある。
同じgame_id/count_seqで読み取り専用の接続から取得する。DB移行・新しい試合は不要。
**必ず観測更新CSVを生成した元のDBを指定する。** キーが一致するだけでは別DBの取り違えを検出できない。

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update \
  --output-dir ../saberbb_finite_pitch_spin \
  --pitch-db path/to/game.db \
  --gyro-sign 0
```

`path/to/game.db` は実際の元DBパスへ置き換える。
欠落・重複・不正な回転値はエラーにする。0回転で黙って補完しない。
文献曲線範囲内の評価対象接触について照合し、使用した一意な投球数と保存スカラー値のハッシュを記録する。
ハッシュは読み取りスナップショット内の使用行を対象とし、更新中のDB本体だけをハッシュしたとは主張しない。

## 3D回転ベクトルへの再構成の仮定

保存スカラーは完全なワールド回転ベクトルではない。
現在の投球コードのマグヌス方向 `(sin(angle),0,cos(angle))` を、保存入射速度に垂直な面へ射影する。
正規化した射影方向をlift、入射速度の単位ベクトルをvhatとして、
`omega_transverse = vhat × lift * rpm*pi/30*efficiency` とする。
真横に−Yへ進む球なら、角度0のバックスピンは−X回転となる。
この射影による3D化は明示的なモデル解釈であり、保存された実測回転軸ではない。

残りのジャイロ成分の大きさは `rpm*pi/30*sqrt(1-efficiency²)`。
向きは効率だけでは決まらないため、次の3ケースを分ける。

| --gyro-sign | 意味 |
|---|---|
| 0 | ジャイロ成分を省略し、横回転成分だけを使う。全回転数は元rpmより小さくなる |
| +1 | ジャイロ成分を入射速度と同方向へ加える。全回転数は元rpmと一致 |
| −1 | ジャイロ成分を入射速度と逆方向へ加える。全回転数は元rpmと一致 |

gyro-signを変えるときは別出力先を指定する。
投球から接触までの回転減衰は未実装。
`--ball-spin-rad-s X Y Z` の固定ベクトル指定も維持するが、`--pitch-db`とは同時指定しない。

## 同じ球でゼロ回転と比較する

DB回転または非ゼロの固定ベクトルを指定した実行では、各球・各モデルについてゼロ回転の応答も計算する。
`finite_barrel_spin_pairs.csv` に、同じ接触のゼロ回転→入力回転のEV/VLA/rpm/エネルギー/地面応答差を出力する。
その分、ゼロ回転だけの実行より計算量は増える。
比較は衝突時の回転だけを変える反実仮想で、無回転の投球が飛んだ場合の軌道を再計算するものではない。
比較は両側成功した共通集合だけ。未評価は0として平均せず、statesとpaired_contactsを区別する。
`stage=source_z_reference_direction / band_2=fair_direction` は基準法線モデルの方向で集合を固定する。

通常のsummary/statusは入力回転を使ったモデルの集計であり、ゼロ回転側の別集計ではない。
summary/examplesには入力回転数・入力ワールド回転ベクトルを追加し、使用値を追跡できる。
metadataはversion=2とし、回転の取得元・再構成方法・ジャイロ仮定を保持する。

次の解析では、まず **finite_barrel_coverage/checks/status/summary/spin_pairs の5 CSV** を確認する。
文献比較のCSVも保持するが、ゲーム分布の診断値と文献の撮影面の値を混ぜて平均しない。
現在のNathanバットに、文献の木製バットex=0.464をそのまま移植して採用しない。

## invalid_pitch_spinで止まった場合

既定の検証は、回転数が非負、回転効率が0〜1、各値が有限であることを要求する。
旧版の一律invalid_pitch_spinを、原因とキー・保存値を含むメッセージへ変更した。
例えば次の形で表示する。

```text
Diagnostic failed: spin_efficiency_out_of_range: game_id=..., count_seq=..., spin_rate=..., spin_angle=..., spin_efficiency=...
```

現行の選手生成コードはspin_efficiencyを正規分布からサンプリングし、その場で0〜1へ制限していない。
投球作成時もその値をコピーしているため、既存DBに範囲外値が入っている可能性がある。
ただし、この生成経路だけでは個々のエラーの原因を確定できない。必ず保存値のレポートで確認する。
今回の修正ではゲームの生成処理・既存DB値・入力回転を変更しない。

DB回転を使うと、出力先にfinite_barrel_invalid_spin.csvを作成する。
問題のgame_id/count_seq、最初に検出したvariant、原因、保存スカラー、入射速度を記録する。
同じ球・同じ原因は1行にまとめる。エラー時もこのレポートは逐次書き込み・flush済みなので読める。
厳密モードで停止した出力先の他の部分ファイルを、完成した診断結果として使わない。

不正スカラーや再構成不能な回転の球を明示的に除外して続ける場合は、次を指定する。

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update \
  --output-dir ../saberbb_finite_pitch_spin_checked \
  --pitch-db path/to/game.db \
  --gyro-sign 0 \
  --invalid-spin-policy exclude
```

途中で作成された以前の出力先は再利用せず、新しい出力先を指定する。
既定は引き続き `--invalid-spin-policy error` である。
excludeでもキー欠落・重複、DBスキーマ不備、元接触CSVの破損は停止する。
これらを不正回転として除外すると、DBの取り違えなどを隠すためである。

除外した接触は全8モデルで `not_evaluated:invalid_pitch_spin` とする。
0回転・EV=0へ置き換えず、spin_pairsの共通成功集合にも含めない。
coverage/statusには除外した試行件数、metadataには不正な一意投球数とレポートのハッシュを記録する。
metadataのused_scalar_rows_sha256とunique_pitchesは、実際に再構成に成功した保存行を対象とする。
除外CSVの行数は同じ球の複数条件をまとめた数なので、除外試行数とは異なる。
metadataはversion=3へ更新した。

除外が多い場合、有効な球だけの結果には選択の偏りが生じ得る。
最終的な回転分布の評価前に、finite_barrel_invalid_spin.csvとcoverageを併せて確認する。
効率を1へ丸める、負の回転数を絶対値へ変える、といった自動修正は行わない。

## 回転効率の生成範囲と新しい試合データ

`PlayerFactory` が正規分布から生成する `spin_efficiency` は、有限値を
`[0, 1]` へ制限します。再抽選はせず、乱数の消費数は変えません。
そのため境界に確率質量が生じます（切断正規分布への変更ではありません）。
NaN・無限大は生成エラーです。

投球作成の共通経路 `create_pitch_for_call` でも、選択した球種の効率が
有限かつ `[0, 1]` にあることを検証し、不正な既存能力値はエラーにします。
保存済みの能力値や投球軌道、診断入力を丸める処理は追加しません。

整合した新規診断には、新しく生成した選手による試合データから、観測更新CSVも
再生成してください。既存選手のまま試合だけを作り直すと、範囲外能力値が残ります。
`--generate` は選手の追加であり、既存選手の更新ではありません。
`--sampleyear` も選手能力値を再生成しません。
既存DBを残し、別の初期化済みDBで新規選手・試合を作る方法を推奨します。

既存データの解析は、従来の `--invalid-spin-policy exclude` と
`finite_barrel_invalid_spin.csv` による明示的な除外・記録を維持します。

## 入射回転による差の分布とEV×VLA

平均差の相殺を確認するため、非ゼロ固定回転または `--pitch-db` を指定した
有限半径診断で、以下の3ファイルを追加出力する。衝突計算そのものは変更しない。

| ファイル | 内容 |
|---|---|
| `finite_barrel_spin_delta_stats.csv` | EV・VLA・回転数の符号付き平均差、平均絶対差、RMS、正・負・厳密ゼロの件数 |
| `finite_barrel_spin_delta_hist.csv` | 同じ3指標の差のヒストグラム |
| `finite_barrel_spin_ev_vla.csv` | ゼロ回転と入力回転のEV×VLA二次元度数・割合 |

差はすべて「入力回転−ゼロ回転」。各scope/variant/modelについて両条件で衝突成功した
接触だけを使い、未評価をゼロ補完しない。比較集合cohortは以下の3種類。

- `all_common`: 全方向の共通成功集合。
- `reference_fair_common`: ゼロ回転のnormal_profileがフェア方向となる共通成功集合。
  各接線モデルの方向による選び直しを避ける比較の主軸。
- `both_fair_common`: 当該モデルの両回転条件でフェア方向となる集合。
  境界をまたぐ打球を除く選択が入るため、主比較とは別に読む。

ゼロ回転・入力回転のEV×VLAは同じ分母paired_contactsを使う。
列fractionは各モデル・集合の共通接触数に対する割合であり、全スイングに対する率ではない。
scope・variant・cohort・modelは重複するため合算しない。
これはフェア方向の接触集合であり、バント除外などを揃えた実測BBE集合ではない。

EV区切りは0〜70 m/sを5 m/s刻み、VLAは−90〜90度を10度刻み。
差の区切りはEVが−2/−0.5/−0.1/0/0.1/0.5/2 m/s、
VLAが−10/−5/−2/−1/0/1/2/5/10度、
回転数が−2000/−1000/−500/−100/0/100/500/1000/2000 rpm。
すべて下限を含み上限を含まない。空の下限・上限はそれぞれ負・正の無限大。
CSVは非ゼロのセルだけを出力し、省略セルは度数0。
正負件数は厳密比較のため、対照モデルの微小な数値誤差にも注意する。
個々の値や分位点は保存せず、既存の平均CSVからこの分布を復元することはできない。

試合や観測更新CSVの作り直しは不要。同じ入力CSVとその元DBを指定して再実行する。

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_finite_pitch_spin_distribution \
  --pitch-db "/path/to/saberbb.db" \
  --gyro-sign 0
```

出力先は未作成のディレクトリを指定する。回転比較を指定しない実行では
3ファイルはヘッダーのみとなる。DBは読み取り専用。ジャイロの仮定、
投球軌道や接触探索を固定する比較、実測未校正という制約は従来どおり。

## Statcastとの比較

入力回転側がフェア方向となる集合 `input_fair_common` を追加した。
実測CSVの選別・バント注釈・欠損監査と共通格子比較は
[Statcast比較手順](statcast_distribution_comparison.md)を参照。
フェア方向と実際のフェア打球の差などを記録する探索比較であり、
比較集合の完全一致や校正完了は主張しない。

## 接触集合の選別監査

全接触・適用可能・モデル成功・フェア方向のEV×VLAと除外理由を追加出力する。
[接触選別診断](contact_selection_diagnostics.md)を参照。
同じ保存基準応答による集合間比較と、同じ集合での入力回転モデル比較を区別する。
端面・曲線範囲外へ有限半径モデルを外挿しない。
