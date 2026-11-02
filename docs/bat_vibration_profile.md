# 文献の打点依存反発係数を使った診断

## 根拠と位置付け

[Nathan (2000), Dynamics of the baseball–bat collision](https://baseball.physics.illinois.edu/AJP-Nov2000.pdf)
のTable I/II・Eq.(18)・Fig.12を使う。
これは特定の木製バットを対象とした計算モデルで、低速実験との比較を含む。
**高速のFig.12の線は実測CORの点列ではない。**
本診断の曲線は図を小数第2位へ概読した折れ線で、著者提供の数値データや弾性モード計算の再実装でもない。
読取の丸め・図の解像度に由来する不確かさがあり、細かなEV差の確定には使わない。

既存のkensrud2017_woodと混ぜず、独立のnathan2000_standard_woodとして扱う。
質量0.885 kg、重心位置0.564 m、重心まわりの回転半径0.23 mを同じ資料から採用する。
慣性はI_cm=M*k²=0.0468165 kg·m²。
節の位置と反発最大点は同義ではなく、一次振動の節をそのまま最大反発点に設定しない。

## 現行のeとの対応

静止バットへの正面衝突で、入射速度に対する反発速度の符号付き比をq、逆有効質量をμとすると、

```text
R = m_ball * μ
q = (e - R) / (1 + R)
e = q*(1 + R) + R
```

qが見かけ反発係数。反跳しなければ負にもなり、絶対値で置き換えない。
一方、Fig.12のe_effはEq.(18)により現行の剛体式のeへ直接対応する。
今回の実装は`solver_e = e_eff(z)`とし、上のq→e変換を重ねない。
e_effへ有効質量補正を再度加えると反動を二重計上する。
既に球の散逸と振動の影響を合わせた係数なので、さらに同じ振動損失を重ねて乗算しない。

保存則チェックの損失項は、この有効eに対応するモデル上の全損失。
それを個別に計測された振動エネルギーと解釈しない。

## 保存接触との座標対応と適用範囲

保存された軸方向打点s=0を、ノブからz=0.71 mへ対応させる。
これは文献の代表衝突位置を使う明示的な対応仮定で、保存入力の実測された振動節ではない。

```text
z = 0.71 + closest_axial_at_entry_m
c = 0.564 - 0.71 = -0.146 m
```

Fig.12からz=0.54～0.84 mの部分を読み取り、この範囲内だけ線形補間する。
範囲外はout_of_curve、保存された球面端部はunsupported_capとして未評価に残す。
どちらも値を端へクリップしたり、EV=0で埋めたりしない。
文献の曲線は横方向の衝突から得られているため、軸方向成分を持つ端部へ一般化しない。

側面の斜め衝突へ法線方向の有効eとして使う点も、今回の診断上の拡張である。
バットの半径・形状、接触探索、予定軌道、L、法線、投球速度、接触点速度は保存値を固定する。
文献バット全体の形状やスイングを再現するものではない。
特にLは保存された有効支点距離で、文献バットの全長や握り位置に置き換えない。

Fig.12の実線と破線をそれぞれfull_speed/half_speedテンプレートとして計算する。
保存衝突の速度に合わせて曲線を再推定・速度補間する機能はない。
full_speedについて、文献条件の局所接近速度を`64+51*(z-0.564)` m/sとして、
保存接触の法線接近速度との比を記録する。half_speedでは分母を半分にする。
速度比は適合性を点検するための値で、速度差を補正済みという意味ではない。
normal_source_speed_ratioの帯は全モデルでfull_speed基準に固定し、比較集合を揃える。
half_speedの平均source_speed_ratioは、その帯分けに使う値の2倍となる。

## 比較するモデル

| model | 質量分布 | e | 対象 |
|---|---|---|---|
| constant_0.6kg | 元の一定有効質量 | 0.5 | 元の接触 |
| kensrud_rigid_0.5 | 前回の文献バット | 0.5 | 元の接触 |
| nathan_rigid_0.5 | 今回の独立プリセット | 0.5 | 曲線適用範囲の側面 |
| nathan_fig12_full_speed | 同上 | 実線のe_eff(z) | 同上 |
| nathan_fig12_half_speed | 同上 | 破線のe_eff(z) | 同上 |

最初の2つは元データ・以前の診断との再現確認用。
新しい3つは同じ適用範囲に揃える。
質量分布を変えた影響はkensrud_rigid_0.5→nathan_rigid_0.5、
損失を入れた影響はnathan_rigid_0.5→各Fig.12曲線として別々に比較する。
元の全接触の平均と、範囲内だけの平均を直接引き算しない。

## 実行

以前の観測更新診断の出力3ファイルを使う。

- observation_update_contacts.csv
- observation_update_metadata.json
- observation_update_coverage.csv

```sh
python3 tools/diagnostics/vibration_profile_probe.py \
  --input-dir ../saberbb_observation_update \
  --output-dir ../saberbb_vibration_profile
```

新しい試合・DB移行・接触の再探索は不要。入力を変更せず、既存出力への上書きを拒否する。
実行中に入力異常で停止した部分出力は結果に使わず、原因を確認して別の出力先へ再実行する。

## 出力と確認順

| 出力 | 内容 |
|---|---|
| vibration_profile_coverage.csv | 元試行数、元接触数、適用可能側面・端部・範囲外・非接近の件数 |
| vibration_profile_checks.csv | 元の衝突再現、反発則・力積・エネルギー収支 |
| vibration_profile_status.csv | variant×モデル×適用可否×衝突状態の件数 |
| vibration_profile_summary.csv | 適用e・速度比・EV/VLA・有効質量とフェア方向の弱打/強打率 |
| vibration_profile_pairs.csv | 共通して評価できる接触、両方フェアのEV、元の強打の維持・変化 |
| vibration_profile_transitions.csv | モデル間の方向×EV帯の遷移 |
| vibration_profile_knots.csv | 実際に使った図の概読点列 |
| vibration_profile_metadata.json | 出典、元PDFのSHA-256、座標対応、形状・速度・モデル化の制約 |

最初は**coverage・checks・status・summary・pairs・transitionsの6 CSV**を添付する。
knotsとmetadataも手元に残し、曲線と前提を追跡できるようにする。

1. 元728430試行・514997幾何接触を再現するか確認する。
2. 元の一定質量とkensrud_rigid_0.5が以前の結果を再現するか確認する。
3. statusで未評価の端部・範囲外を把握する。
4. all_swings / observed_sigma_0.005_r0.9 / overallを基準に、剛体対照→打点依存のpairsを見る。
5. signed_axial、source_z、baseline_impact、normal_source_speed_ratioで傾向を分ける。

pairsの元の強打集合は、**両モデルが評価できた接触**のうち元モデルでフェアかつEV>=40の集合。
適用できなかった球を強打喪失へ加えない。
両方フェアとなる集合のEV、強打→弱打/中間/強打/非フェアの件数を確認する。
弱打はEV<25、強打はEV>=40。ファウル方向は最終試合結果とは別。
未評価を含む分母と、力積接触・フェア方向の分母を区別する。
scope・variant・stage・モデルは同じ試行の別の見方なので合算しない。

この文献曲線で弱打が十分生まれると決めつけない。
不足が残る場合も、ゲームの弱打率へ合わせて曲線を下げず、
打点分布・接触点速度・法線応答・保存幾何の範囲を次の検討対象として判断する。
