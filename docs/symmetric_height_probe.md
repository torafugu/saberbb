# 同一球の上下対称の高さ誤差とバット軸の診断

## 目的と対照の範囲

前段の速度固定対照では、平均VLA低下はほぼ残った。今回は同じ球に上下対称の
高さ誤差を与え、狙い位置から法線・VLAへの変換、高さによる予定軸の変化、接触の選別を確認する。

**バット速度を全条件で厳密に固定するため、一定速度で平行移動する有限接触区間を使う。**
バット軸も各試行内では回転させない。球も従来の局所探索と同じ定速度近似を使う。
この局所直線運動は幾何を切り分ける診断対照であり、本体や従来の回転モデルの置き換えではない。
前段の回転モデルと接触数・VLAが同じになることは要求しない。

## 各球の共通基準

保存した3時点の正確な観測から、従来と同じ一定加速度推定で到達位置・時刻を計算する。
この誤差なし対照を全高さ条件の共通の原点にする。真の到達位置・時刻との一致を検証し、
1e-8m/秒を超える不整合はstatusへ理由を記録して除外する。
推定器へ実際の最終位置や到達時刻を入力して予測を作ることとは区別する。

基準となるスイング計画は既存の`plan_swing()`で作る。
予定attack angleと速度は保存済みの打者設定、実行角度誤差は保存済みの値を使う。
実行角度誤差は全高さ条件で固定し、新コースに応じて再計算しない。
追加の乱数・観測ノイズ・旧timing_offsetは使わない。

各球・レバー長において、次を全条件で共有する。

- 基準計画の予定時刻。
- 球の位置・速度、接触区間の半長0.16m、有効半径0.07m。
- 基準計画の実行角度誤差を含む`actual_velocity_world_ms`という世界座標の速度ベクトル。
- 反発係数0.5、有効質量0.6kg、探索範囲±60ms。

## 比較する条件

高さ誤差は`0, ±0.01, ±0.03, ±0.05m`。正は実球より高めを狙う方向。
両軸条件とも、基準のactual_point_world_mへ同じworld-Z誤差だけを加える。
X/Yアンカーを変えず、実行角度誤差による基準位置のずれもそのまま保持する。

| axis_mode | 軸の決め方 |
|---|---|
| fixed_axis | 誤差ゼロの基準計画の実行後の軸を固定 |
| height_dependent_axis | ずらした予測高さから予定軸を計算し、同じ実行角度誤差の回転を適用 |

高さ依存の予定角は既存の規則を使う。

```text
normalized_height = (predicted_z - 0.825) / 0.325
planned_angle_deg = clamp(30 - 15*normalized_height, 10, 60)
```

軸条件間でアンカーと速度は変えない。
共通支点の変更による追加の位置ずれを混ぜず、軸の規則だけを比較する。
これは変更後の全スイング計画を再現するものではない。

各試行の姿勢と接触点速度は次のように定義する。

```text
point(T) = shifted_baseline_point + fixed_world_velocity * (T - nominal_time)
axis(T) = selected_axis
contact_point_velocity = fixed_world_velocity
```

したがって軸方向位置が変わっても、衝突に使う速度は固定される。
前段の「同じ接触イベントで速度だけ変更」と違い、今回は狙い位置・軸が変わるので
接触探索からやり直す。両方接触・片側だけ接触・両方非接触を分けて記録する。

## 実行

追加のDB移行や試合生成は不要。既存の24958～25317のDBを使える。
履歴を持たない古い行は補完せず、statusにmissing_observation_history等を記録する。

まずL=0.9mで評価する例：

```sh
python3 tools/diagnostics/symmetric_height_probe.py --db "/path/to/saberbb.db" --first-game-id 24958 --last-game-id 25317 --levers-m 0.9 --output-dir ../saberbb_symmetric_height
```

IDはDBに存在する範囲を指定する。出力先は未作成のディレクトリ。
L=0.9mのみなら1球14試行、48,562球なら679,868試行。
`--levers-m`を省略するとL=0.6/0.9/1.2mで1球42試行。
`--heights-m 0.01 0.03 0.05`で正の誤差幅を指定でき、ゼロと負の側は自動追加する。
DBは読み取り専用・query_onlyのトランザクションで開く。

## 出力と分母

| ファイル | 用途 |
|---|---|
| symmetric_height_coverage.csv | 全キー、有効履歴、1球の試行数、全試行数 |
| symmetric_height_status.csv | 有効・欠損・不正履歴の件数 |
| symmetric_height_checks.csv | 誤差なし予測の残差、速度固定・アンカー移動・ゼロ軸一致、鉛直速度分解の残差 |
| symmetric_height_summary.csv | 各高さ・軸の探索状態、接触・cap・非接近、EV/HLA/VLA・法線・鉛直速度 |
| symmetric_height_symmetry_pairs.csv | 同じ球の低め→高め比較、誤差ゼロも含む中心値のずれ |
| symmetric_height_axis_pairs.csv | 同じ球・同じ高さ誤差での固定軸→高さ依存軸の比較 |
| symmetric_height_transitions.csv | 上下比較・軸比較の探索状態と衝突状態の遷移 |
| symmetric_height_metadata.json | 対照の運動・原点・軸・速度・係数・探索の定義 |

scope=all_swingsは全診断キー、recorded_batting_onlyはUnrecordedBattingを除く。
重複するscopeを合算しない。summaryのstatesは元のキー数、contactは幾何のある試行数。
未収束・開始時重複は非接触と分ける。EV/HLA/VLAと鉛直速度3成分は力積正の接触だけの平均。
法線・軸方向位置・法線速度比・接触時刻は接触幾何がある集合の平均。
summaryの条件間では接触した球の集合が違う可能性がある。

symmetry_pairsのheight_error_mは正の誤差幅。beforeは低め（−d）、afterは高め（＋d）。
axis_pairsではheight_error_mは符号付き、beforeはfixed_axis、afterはheight_dependent_axis。
axis_pairsのaxis_mode欄は比較名fixed_to_height_dependentを持つ。

pairsの`paired_impulse_contacts`は両端とも力積正の球数。
symmetry_pairsの`triple_impulse_contacts`は、さらに同じ球の誤差ゼロでも力積正の球数。
各変数に有効なpairs/triples件数を付ける。ゼロで接触できなかった球を勝手に0で補わない。

```text
center_shift_vla = (VLA_lower + VLA_upper)/2 - VLA_zero
```

`center_shift_*`、`zero_*`、`triple_before_*`、`triple_after_*`は同じ三者接触集合で計算する。
単なる上下の符号反転ではなく、誤差ゼロから上下の中点がどれだけずれたかを測れる。
通常のbefore/afterは二者接触集合なので、三者集合と件数が異なる場合は混ぜない。
HLA差は±180°で折り返し、HLAの中点は循環角の曖昧さを避けて出力しない。
axis_pairsには誤差ゼロとの三者比較がないため、triple関連列は0/空欄とする。

## 確認順と解釈

1. coverage/status/checksで記録の充足・誤差なし原点・速度とゼロ軸の一致を確認。
2. 誤差ゼロで2軸条件の結果が一致するかを確認。
3. transitionsで片側だけ接触する球の数を確認。summary平均だけで対称性を判定しない。
4. symmetry_pairsの同じ三者集合で、VLA・法線Z・鉛直力積のcenter_shiftを見る。
5. axis_pairsで、固定軸から高さ依存軸への変更がそのずれをどれだけ変えるかを確認。

固定軸でもずれが残れば、球の下降・基準の攻撃角・有限区間・衝突式などの性質が候補。
高さ依存軸で追加のずれが大きくなれば、予定軸の高さ依存が関与する。
上下非対称だから直ちにバグということではない。法線やVLAへ補正係数を追加する前に、
どの条件で非対称になるかを特定する。診断上の接触率を本体のContact%として扱わない。

## 検証

水平な投球・バットの解析例で、上下のEV一致、法線Z/VLAの符号反転、中点のゼロ一致を確認。
固定速度・アンカー移動、ゼロ軸一致、角度clamp、非接触、二者/三者の分母、
欠損履歴・空範囲、DBのSHA-256不変性を検証する。

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
python3 -m unittest discover -s tests/sql -p 'test_*.py'
```

実行後はmetadata以外の7つのCSVを添付する。大きな全試行明細は生成しない。
