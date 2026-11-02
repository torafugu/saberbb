# 予測誤差から接触幾何・打球品質への診断

## 目的と実行

観測更新診断で残った高EVへの集中と、観測誤差を増やしたときのVLA低下を調べる。
予測誤差→接触位置→法線・相対速度→EV/HLA/VLAを同一球の比較と段階別集計で追う。
本体の物理計算・スイング判断や診断の探索・係数を変更しない。

前回の出力ディレクトリにある次の3ファイルを読み取り専用で使う。

- `observation_update_contacts.csv`：全試行の既存明細。
- `observation_update_metadata.json`：係数・形状・条件。
- `observation_update_coverage.csv`：欠けた行を検知する元の件数。

```sh
python3 tools/diagnostics/forecast_contact_geometry.py --input-dir ../saberbb_observation_update --output-dir ../saberbb_forecast_geometry
```

出力先は新規ディレクトリ。**24958～25317を診断した前回の出力をそのまま使える。**
試合の再生成・DB移行は不要。先に添付した7つの集計CSVだけでは接触明細を復元できないため、
ローカルに残っている全明細を入力する。もし全明細を削除していた場合も、新試合を作らず、
同じDB・同じ試合IDで`observation_update_probe.py`を新規出力先へ再実行すればよい。

## b/Rの定義と適用範囲

Rはバット半径＋球半径に相当する有効接触半径（今回0.07m）。
bは球中心の局所的な相対直線がバットの無限軸へ最接近する際の符号付き距離。
**回転軌道全体を探索して得た最小距離ではない。**

接触時の単位軸a、単位法線n、球と接触点の相対速度uを固定し、次で求める。

```text
u_perp = u - a*dot(u,a)
d = -u_perp / |u_perp|
tangent = cross(a,d)
r_entry = (R + entry_clearance) * n
local_b_over_r = dot(r_entry,tangent) / R
```

`entry_clearance`は最初に接触判定された地点の数値許容誤差による隙間。
既存の`min_evaluated_clearance_m`は、接触以前の全地点で許容誤差を超えているため、
接触試行ではこの入口の隙間と一致する。

- 側面接触だけに適用する。端の丸い部分（cap）は別分類とし、b/Rを与えない。
- 相対速度の軸直交成分が0の場合も未定義。
- 符号は軸と相対速度で決まる座標系の向きであり、上下の符号ではない。
  左右打ちなどで軸の向きが変わるので、集計には絶対値を使う。
- b/R≈0は局所的に軸へ向かう接近、|b/R|≈1はかすめる接近を表す。
- **接触入口の球中心と軸の距離/Rとは違う。** 入口の距離/Rはほぼ1でも、
  相対運動の最接近距離を表す局所b/Rは0～1に分布しうる。
- 非接触では入口の法線・相対速度が存在しないため、b/Rを捏造しない。
  非接触の内訳は予測X/Z誤差・時刻誤差帯で確認する。

軸方向の位置は`closest_axial_at_entry_m / half_length_m`で別に扱う。
半長0.16mの有限接触区間における中央からの位置で、バット全長の割合ではない。
既存の直線・無限円柱診断のb/Rと、この回転接触時点で固定した局所b/Rは区別する。

## 出力

| ファイル | 用途 |
|---|---|
| forecast_geometry_coverage.csv | 読み取り行数、球×レバー長のキー数、比較ペア数 |
| forecast_geometry_status.csv | 各条件の探索・衝突状態 |
| forecast_geometry_checks.csv | 保存された法線相対速度・力積・EV/HLA/VLA等の再構成残差 |
| forecast_geometry_chain.csv | 下表の段階別同時集計と打球品質・鉛直速度の内訳 |
| forecast_geometry_pairs.csv | 旧予測と各条件の双方接触した同一球のEV/HLA/VLA比較 |
| forecast_geometry_geometry_pairs.csv | 同一球の局所b/R・軸方向位置・法線Z・鉛直速度等の比較 |
| forecast_geometry_transitions.csv | 接触・非接触・未収束等の同一球状態遷移 |
| forecast_geometry_metadata.json | 元の仮定、入力CSVのSHA-256、局所b/Rの定義 |

`chain.csv`のband_1～3はstageごとに意味が変わる。

| stage | band_1 | band_2 | band_3 |
|---|---|---|---|
| overall | all | all | all |
| forecast_x_z | 予測X誤差（符号付きm） | 予測Z誤差（符号付きm） | all |
| forecast_time | 時刻誤差の絶対値（ms） | all | all |
| forecast_z_sign | 予測Z誤差（符号付きm） | all | all |
| error_to_impact | 予測X/Z誤差の合成距離（m） | 局所\|b/R\| | 軸方向位置の絶対割合 |
| impact_to_normal | 局所\|b/R\| | 軸方向位置の絶対割合 | 法線方向の相対速度比 |
| normal_to_outcome | 法線方向の相対速度比 | 法線Z成分（−1～1） | 接触点バット速度（m/s） |

予測誤差は予測−実際。正のZ誤差は高めを狙う計画。
誤差ゼロ近辺は絶対値1e-8m未満を専用帯とし、丸めの符号で上下に分けない。

`scope=all_swings`は全診断キー、`recorded_batting_only`はUnrecordedBattingを除く。
2つは重複するので合算しない。各stageも同じ試行の別の集計であり合算しない。
`overall`、予測誤差帯、`error_to_impact`は非接触も含む。
後段の`impact_to_normal`と`normal_to_outcome`は接触幾何のある球だけ。
`contact_pct`の分母は行のstates、打球品質の分母は力積正の接触。
法線・局所b/R・HLA・VLAの有効件数を別々に出す。未定義を0と扱わない。
法線・相対速度の平均は接触幾何がある集合、鉛直速度3成分の平均は力積正の集合を使う。

ペアCSVは全診断キーを含む。`pairs.csv`と`transitions.csv`のadjusted_contact_type=allは
分類別行の合計なので合算しない。`geometry_pairs.csv`はレバー長・比較条件ごとの集計で、
各変数に有効なペア件数を付ける。HLA差は±180°へ折り返す。

## 高EV・VLA低下の確認順

1. coverage/status/checksで元の件数と整合すること、再構成残差が小さいことを確認。
2. L=0.9m・overallで前回の接触数・EV/VLA・ファウル方向率を再現する。
3. error_to_impactで、誤差幅の増加がb/R・軸方向位置・cap・非接触のどこへ伝わるかを見る。
4. impact_to_normalで、芯を外れた局所接触が法線方向相対速度を落としているかを見る。
5. normal_to_outcomeで、同じ法線速度比・接触点速度におけるEV/VLAを見る。
6. forecast_z_signと同一球のgeometry_pairsで、Z誤差と法線Z・鉛直力積・VLA変化を確認する。

各接触の鉛直速度は、既存の衝突式から次に分解できる。

```text
outgoing_z = incoming_z + impulse_delta * normal_z
```

出力の`avg_incoming_z_ms`、`avg_impulse_z_ms`、`avg_outgoing_z_ms`で内訳を見る。
同じ力積正の集合では平均にもこの加法関係が成立する。
VLAは鉛直・水平速度から非線形に計算するため、平均速度から平均VLAを再現してはいけない。
EV/HLA/VLAは元の1つの打球速度ベクトルから再計算して照合する。

同時集計は関連を見るもので、単一要因の因果効果を証明しない。
接触できた球だけの平均には選別があるため、pairsの同一球比較と状態遷移も確認する。
ここで反発係数・有効質量・観測幅を調整せず、どの変換段階に偏りがあるかを特定する。

## 検証と対応範囲

現行の観測診断version=1、接触contract=1、e=0.5/M=0.6kgに対応する。
入力メタデータ・件数・キー重複・条件の欠損・形状の不整合や、保存ベクトルの再構成不一致は
エラーで停止する。不正行を黙って分母から落とさない。
キー照合には一時SQLiteファイルを使い、終了時に削除する。元DBやCSVを書き換えない。

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
python3 -m unittest discover -s tests/sql -p 'test_*.py'
```

初回評価にはcoverage/status/checks/chain/pairs/geometry_pairs/transitionsの7つのCSVを添付する。
元の巨大なcontacts.csvやinputs.jsonlは添付不要。
