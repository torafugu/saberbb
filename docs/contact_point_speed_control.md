# 接触点速度を計画の基準速度に固定する対照診断

## 目的

前段の診断では、観測誤差を増やすと平均VLAが低下し、同じ球でも上向きの鉛直力積が
小さくなった。今回は保存済みの同じ接触イベントで、バット速度だけを差し替えて
衝突を再計算する。回転による接触点速度の変化の寄与を切り分けるための対照である。

| velocity_mode | 衝突に使うバット速度 |
|---|---|
| actual_point_velocity | 元の接触時刻・軸方向位置における接触点速度 |
| planned_reference_velocity | スイング計画の予定時刻における基準打点の速度 |

基準速度には`plan.actual_velocity_world_ms`を使う。
これは予定時刻（nominal_time_sec）、接触区間中央（軸方向位置0）での速度で、
**実行角度誤差を反映した速度**である。理想的な実行誤差ゼロの速度ではない。
同じ計画の次の値から独立に再構成して照合する。

```text
reference_velocity = pivot_velocity + cross(omega, L * actual_axis_at_nominal_time)
```

現行の接触点速度も計画から再構成する。

```text
phase = entry_since_release_sec - nominal_time_sec
axis_at_entry = rotate(actual_axis_at_nominal_time, omega, phase)
actual_point_velocity = pivot_velocity
                        + cross(omega, (L + closest_axial_at_entry) * axis_at_entry)
```

軸・速度・位相時計の再構成が一致しない場合は停止する。
これにより、異なる試合・条件の計画ファイルとの誤結合も検知する。

## 固定するものと変えるもの

接触時刻、接触位置を表す軸方向距離、単位軸、法線、投球速度、反発係数0.5、
有効質量0.6kgは固定する。衝突関数へ渡すバット速度だけを変える。
予定角・狙い位置や観測誤差を再生成せず、接触探索もやり直さない。

これは、変更した速度で同じイベントが発生する軌道を作る実験ではない。
**同じ接触イベントにおける衝突計算だけの対照**である。
非接触は両条件とも非接触のまま記録し、新たな接触率・打席結果を算出しない。
速度を差し替えた結果、相対運動が非接近になる場合は`non_approaching`へ遷移させる。
その場合に打球EV/HLA/VLAを無理に付けない。

また、基準速度への固定では、軸方向位置による速度差と、接触時の回転位相による速度差を
同時に除く。今回だけで両者を個別に分離したとは主張しない。

## 実行

24958～25317を診断した**観測更新の元の出力ディレクトリ**を指定できる。
前段のforecast_geometryディレクトリではない。

必要なファイルは次の4つ。

- `observation_update_contacts.csv`
- `observation_update_inputs.jsonl`
- `observation_update_metadata.json`
- `observation_update_coverage.csv`

```sh
python3 tools/diagnostics/contact_point_speed_probe.py --input-dir ../saberbb_observation_update --output-dir ../saberbb_contact_speed
```

出力先は未作成ディレクトリ。新しい試合・DB移行は不要。
元のファイルを削除していても、同じDB・試合IDの範囲で観測更新診断を再実行すればよい。
元CSV/JSONLは読み取り専用。キー照合には一時SQLiteファイルを使い、終了時に削除する。
メタデータに全入力ファイルのSHA-256を記録する。

## 出力と分母

| ファイル | 内容 |
|---|---|
| contact_speed_coverage.csv | 計画行数、元試行数、接触幾何のある行数、2条件への展開件数 |
| contact_speed_status.csv | 元の探索状態×速度条件×衝突状態の件数 |
| contact_speed_checks.csv | 元の衝突再計算、計画の基準速度、接触時の軸・速度・時計、鉛直速度分解の残差 |
| contact_speed_summary.csv | 各条件のEV/HLA/VLA、使用バット速度XYZ、法線速度比、鉛直速度の平均 |
| contact_speed_pairs.csv | 同じ接触イベントで双方とも力積正のペアに限った、前後・符号付き差・絶対差 |
| contact_speed_transitions.csv | 元の探索状態を保持した衝突状態の前後遷移 |
| contact_speed_metadata.json | 入力ハッシュ、元の仮定、基準速度と対照の定義 |

coverageの`trial_rows`は元の全試行で、`collision_trial_rows`はその2倍。
`geometry_contacts`は元の探索でcontactだった件数で、両条件の衝突計算対象になる。
ここには、差し替え後に非接近へ変わるイベントも含む。

summaryは各条件で力積正となった集合の打球平均を示す。
法線速度比・相対法線速度・使用バット速度の平均は、非接近も含む全接触幾何の集合。
鉛直速度の3成分は力積正の集合で揃える。

```text
outgoing_z = incoming_z + impulse_z
impulse_z = impulse_delta * fixed_normal_z
```

法線Zと投球速度は固定されるので、同じイベントで鉛直力積が変わる部分は
バット速度の差し替えによる法線相対速度・力積の変化である。
平均速度から平均VLAを計算し直してはいけない。角度は各打球の速度ベクトルから計算する。

pairsのbeforeはactual_point_velocity、afterはplanned_reference_velocity。
両条件とも力積正の同一イベント集合で平均と差を計算する。
各変数のペア件数を明示し、HLAの差は±180°に折り返す。
HLAの前後平均は算術平均なので、方向変化の大きさには折り返した差を使う。
非接近への遷移があればsummaryの単純比較とpairsを区別する。

## 集計段階

| stage | band_1 | band_2 |
|---|---|---|
| overall | all | all |
| forecast_z | 元の予測Z誤差の符号付きm帯 | all |
| baseline_impact | 元の局所\|b/R\|、またはcap/未定義 | 元の軸方向位置の絶対割合 |
| baseline_normal | 固定された法線Zの帯 | 元の法線方向相対速度比 |

帯分けは**元の条件で固定**し、対照の結果で帯を移動させない。
b/Rも相対速度に依存するため、対照の新しいb/Rを同じ値だと主張しない。
ここでは前段で得た元の局所b/Rを比較対象のラベルとしてだけ使う。
capと側面は区別し、非接触へb/Rや軸方向位置を付けない。

scope=all_swingsは全キー、recorded_batting_onlyはUnrecordedBattingを除いた集合。
両scopeと各stageは重複するため合算しない。

## 確認順

1. coverageで計画と明細が1対1で結合でき、2条件に展開できたかを確認。
2. checksで元の値・計画からの軸と速度・位相時計の残差を確認。
3. actual_point_velocityのoverallが前回の集計を再現するかを確認。
4. transitionsで、速度固定により非接近へ変わった件数を確認。
5. pairsのoverallで、同一イベントのEV/VLA・鉛直力積の変化を確認。
6. forecast_zで、高め/低めを狙った場合のVLA差が速度固定で縮むかを見る。
7. baseline_impact/normalで、どの接触幾何に変化が集中するかを見る。

VLAの差が縮めば接触点速度の寄与が示される。残った差を法線・接触幾何側の検討へつなぐ。
ここから、どの速度モデルを本体に採用すべきかが自動的に決まるわけではない。
反発係数・有効質量・観測幅は変更しない。

## 検証

解析的な中央接触で2条件が一致すること、軸方向の速度差が衝突式どおりに伝わること、
回転位相・軸・速度の再構成、非接近への遷移、未定義値、元の平均値の再現、
キー欠損・重複・計画の破損の拒否、入力ファイルの不変性をテストする。

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
python3 -m unittest discover -s tests/sql -p 'test_*.py'
```

実行後はcoverage/status/checks/summary/pairs/transitionsの6つのCSVを添付する。
巨大な元明細・計画JSONLの添付は不要。
