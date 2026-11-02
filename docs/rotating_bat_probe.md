# 回転バット軌道と直線移動のオフライン比較

## 目的と実行

保存状態v1を使い、旧length miss／thickness missについて、固定軸の直線移動と
回転する有限接触区間を比較する。両モデルの接触区間・有効半径・探索時間・数値許容差は同じ。
各モデル内で、同じ球のタイミングだけ0にした条件も比較する。
本体の接触採否・乱数・試合結果・DBは変更しない。

既存の24112～24471の状態を再利用できる。DB更新や新しい試合生成は不要。
Python標準ライブラリのみで動作する。未作成の出力ディレクトリを指定する。

```sh
python3 tools/diagnostics/rotating_bat_probe.py --db "/path/to/saberbb.db" --first-game-id 24112 --last-game-id 24471 --output-dir ../saberbb_rotating_bat
```

既定値は感度比較用の仮定であり、実測値やContact率に合わせた推定値ではない。

| 項目 | 既定値 | 引数 |
|---|---|---|
| 回転中心から基準打点までの距離L | 0.6、0.9、1.2 mの3条件 | --levers-m 0.6 0.9 1.2 |
| 軸方向の接触区間 | 基準打点から±0.16 m | --half-length-m 0.16 |
| 有効接触半径R | 0.07 m（バット＋球の半径） | 固定 |
| 探索区間 | t_ref前後60 ms | --half-window-ms 60 |
| 接触の空間許容差 | 1 µm | --contact-tolerance-m 0.000001 |
| 各探索の反復上限 | 4,000 | --max-iterations 4000 |

Lはバット全長ではない。有限区間もバット全体の形状ではなく、旧長さ閾値に対応する
基準打点周辺の比較用区間。両端を球形に丸めた有効カプセルなので、球の中心は区間端から
軸方向に最大Rだけ外でも接触しうる。以前の「軸位置±0.16m」フラグとは同義ではない。
探索開始はリリース時刻より前にはしない。

## 軌道の定義と基準時刻での整合性

保存した理想打点位置p0、単位バット軸a、打点速度vを、**タイミング誤差0の基準時刻**の
位置・軸・速度として使う。保存された軸をこのように解釈すること自体もモデル上の仮定。
保存済みの角速度0から実際の回転中心や角速度を復元できたわけではない。

```text
pivot0 = p0 - L*a
omega = cross(a, v)/L
pivot_velocity = a*dot(a,v)

s = t - timing_offset
axis(s) = RodriguesRotation(omega*s)*a
pivot(s) = pivot0 + pivot_velocity*s
point(s) = pivot(s) + L*axis(s)
```

tはt_refからの時刻差、正のtiming_offsetは遅れ。
回転中心は一定速度で並進し、その周りで固定角速度の回転をする。
vの軸方向成分は回転だけでは表せないため、回転中心の並進に割り当てる。
これにより、タイミング0・t=0で位置p0、軸a、速度vがすべて一致する。
整合誤差を各行・集計に記録する。

直線条件は`point(s)=p0+v*s`、軸a固定であり、既存状態v1と一致する。
回転条件の遅延後の姿勢・速度は、直線条件の遅延後の値とは一致しない。
その差を`phase_position_difference_m`と`phase_velocity_difference_ms`に記録する。
基準時刻での一致と、タイミングを適用した後の差を混同しない。

ボールは保存した本塁面位置と速度による等速直線運動。
本塁面速度自体は一定加速度推定に由来し、ここで投球軌道を再積分してはいない。
時間を±60msへ延長する妥当性も今後確認する必要がある。

## 接触探索と未解決ケース

各時刻で球中心から有限線分への最短距離を求め、Rを引いた値をclearanceとする。
接触可能な任意の線分上の点に対し、相対速度の大きさを上から抑える値は次の通り。

```text
linear_bound = |ball_velocity - v|
rotating_bound = |ball_velocity - pivot_velocity| + |omega|*(L + half_length)
```

clearance/bound以内は真の接触を飛び越えないため、その90%ずつ時刻を進める。
固定時間刻みで交差を判定する方式ではない。clearanceが許容差以下になった時点を報告する。
1µm以内の近接も接触候補に含むため、厳密な接触時刻や接触率とは区別する。

| status | 意味 |
|---|---|
| contact | 探索開始後にclearanceが許容差以下になった |
| no_contact | 区間終端まで接触候補がなかった |
| overlap_at_start | 開始時点ですでに許容差以内。区間内での初回接触とは判断しない |
| unresolved | 反復上限または時刻の数値停滞。非接触には数えない |

接線に近い軌道などで未収束なら、unresolvedを残す。
`min_evaluated_clearance_m`は評価した点の最小値であり、区間全体の厳密な最小距離ではない。
真の接触は許容差到達より後なので、空間許容差をそのまま時刻精度へ換算しない。
特に接線近傍は時刻誤差が大きくなりうる。

## 出力と読み方

| ファイル | 内容 |
|---|---|
| rotating_metadata.json | 仮定した形状・半径・探索区間・許容差・反復上限・衝突係数 |
| rotating_coverage.csv | 対象キー数、有効状態数、1状態あたりの条件数、明細行数 |
| rotating_state_status.csv | 旧分類別の状態欠損・無効・未対応・有効件数 |
| rotating_summary.csv | 旧分類×旧miss原因×タイミング帯×モデルの接触・未解決件数とEV/VLA |
| rotating_transitions.csv | 同じ球の有限直線条件→各回転条件のstatus遷移 |
| rotating_contacts.csv | 有効状態1球×1モデルの姿勢・速度・時刻・法線・0タイミングとの比較 |

キーの和集合、保存状態のバージョン検証・独立再構成はcontact_state_probeと共通。
欠損・無効状態は理由を出し、UnrecordedBattingは独立した分類に残す。
旧miss原因は保存した旧X/Zから再構成し、前段のstate_miss_summaryで不一致0を確認しておく。
`timing_band=all`は全帯域合計なので各帯域と合算しない。

最初にcoverage、欠損、unresolvedとoverlap_at_startの件数を確認する。
`contact_pct_of_valid`は未解決や開始時近接も含む全有効状態が分母。
両タイミング条件がcontact/no_contactまで解決した球だけを`resolved_phase_pairs`として、
0タイミングから実タイミングへの接触増減を数える。
entry時刻平均の分母は`timed_states`（開始時近接を含む）、時刻差平均の分母は
両条件ともcontactとなった`paired_contact_times`。
`linear_infinite_*`は以前の無限円柱・時間窓なしの参考結果であり、有限直線条件と区別する。

contactとなった時刻の線分上の最近点の速度と法線で、反発係数0.5・有効質量0.6kgの
摩擦なし衝突を試す。この係数も仮定で、回転慣性・軸位置に応じた有効質量は未実装。
法線方向では、線分上の点からバット表面までの回転速度差は寄与しない。
開始時近接には初回接触法線を確定できないのでEV/VLAを生成しない。
接近して正の力積を受ける球のみをEVの平均対象にする（impulse_contacts）。
HLA/VLAが定義される件数も別に出す。速度名の末尾`_ms`はm/s、時刻は`_ms`の場合ミリ秒。

各条件のEV/VLAはその条件で接触した部分集合であり、同一球集合の打球品質比較ではない。
同じ球を比較したい場合は明細をgame_id/count_seqで対応させ、両条件contactかつ力積正に限定する。
ContactTypeは旧分類をラベルとして保持するだけで、新モデルの品質分類ではない。
探索窓を狭くしてContact率へ合わせる用途には使わない。

## 検証と次の判断

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
python3 -m unittest discover -s tests/sql -p 'test_*.py'
```

基準打点の位置・速度・軸、数値微分、左右鏡映、有限区間端、速度上界、直線の解析的接触時刻、
空間許容差縮小、探索予算不足、独立した細かいサンプルとの比較、DB不変性・旧データ・空範囲を確認する。
同じ時間窓のまま、回転半径により接触可否が変わる人工例も確認する。
これらは数式・実装の検証であり、実際の野球の回転中心やContact率の妥当性を証明するものではない。

実データでは3半径の感度と、旧miss群の接触→非接触遷移、タイミング0との差を比較する。
未解決が残る場合は反復上限を増やし、必要なら許容差を1e-7mへ縮めて別出力先で比較する。
本体適用の前には、打者のスイング幾何・投球速度推定・狙い誤差の座標の妥当性を検討する。

## 共通3D接触仕様と品質集計

probe v2は[3D接触仕様](contact_3d_contract.md)の共通処理からEV/HLA/VLAを生成し、
球の入力・出力速度XYZと品質記述量を明細へ追加する。
`contact_3d_status.csv`と`contact_3d_quality.csv`も出力する。
既存のprobe v1 CSVをDBなしで検証・集計するコマンドも同仕様に記載している。
