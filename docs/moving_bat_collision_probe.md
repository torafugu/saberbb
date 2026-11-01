# 同一接触での移動バット衝突診断

既存DBを読み取り専用で開き、同じ試合ID範囲から次の2つを一度に出力する。

1. ファウル方向・最終結果・捕球経路の診断（全接触対象）。
2. 比較入力がそろった同じ接触に対して、現行のHLA/VLA/EVと移動バットの法線衝突モデルを比較。

試合シミュレーターの挙動、乱数、保存済みデータは変更しない。
Python標準ライブラリだけで実行でき、DB移行や試合の再生成も不要。
この診断モデルを試合結果へ適用するコードは含めない。

## 実行

リポジトリのルートで実行する。DBパスと試合IDの数値は実データに置き換える。
範囲は両端を含み、**前回28,502接触を集計したafter範囲と同じ範囲**を指定する。
添付例の23689だけを指定した117接触の範囲とは区別する。

```sh
python3 tools/diagnostics/contact_collision_probe.py --db /path/to/saberbb.db --first-game-id 100 --last-game-id 200 --output-dir ../saberbb_after_collision_probe
```

100～200は構文例であり、実際のafter範囲ではない。
Windowsで`python3`がない場合は`python`に読み替える。
パスに空白がある場合は引用符で囲む。出力先は新しいディレクトリを指定し、
既存ディレクトリや前回の出力を上書きしない。
DBの必要列は現在のmainのスキーマを前提とする。

デフォルトは反発係数0.3 / 0.5 / 0.7と、有効バット質量0.3 / 0.6kgの6組。
感度を見るための仮の値であり、現実の打球データで校正した値ではない。
任意の組み合わせに変えるには次のオプションを追加する。

```sh
--restitution 0.3 0.5 0.7 --effective-mass-kg 0.3 0.6
```

## 出力と読む順序

| ファイル | 内容 |
|---|---|
| coverage.csv | 指定範囲、観測された最小・最大試合ID、投球数、接触数、比較可能数、除外数、符号付き厚みの不一致数 |
| direction_coverage.csv | 前回SQLの1番目。入力不足の接触も含む全接触の方向・結果件数 |
| direction_transitions.csv | 前回SQLの2番目。全接触の方向・結果・捕球方法の遷移表 |
| direction_gaps.csv | 前回SQLの3番目。方向と結果に差がある接触の全件集計 |
| excluded_inputs.csv | 衝突比較できなかった入力不足・異常値の理由別件数 |
| collision_summary.csv | 接触種別×現行／診断6組の件数・平均EV/VLA・平均絶対HLA・ファウル方向率・後方方向率 |
| collision_distribution.csv | 接触種別×モデル×EV帯×VLA帯×打ち出し方向の同時分布 |
| collision_contacts.csv | 同一接触ごとの入力、法線、バット速度、衝突後速度、HLA/VLA/EV。現行を含む7行／比較可能接触 |

最初に`coverage.csv`で対象範囲と接触数を確認する。前回と同じデータなら接触数は28,502。
投球数や接触数が一致しなければ、範囲や追加生成の有無を確認する。
方向遷移表の件数合計は入力接触数と一致し、追加送球による二重計上はない。
守備行なしの接触もLEFT JOINで保持する。

次に`direction_gaps.csv`で以下を確認する。

- ファウル側の最終方向なのにGroundRuleDoubleとなる接触が残っていないか。
- backward→fairが残る場合の接触種別・結果・件数。
- ファウルフライ捕球によるOutと、ゴロ処理のOut/DoublePlayを分ける。

最後に衝突比較を確認する。全モデルのcurrentと診断の分母は同じ比較可能接触で統一する。
入力不足の接触は全variantから除外し、`excluded_inputs.csv`に理由を残す。
`defined_hla_contacts` / `defined_vla_contacts`は角度が定義できる件数。
速度ゼロや鉛直方向では角度が未定義になり得るため、方向率は定義できる接触数を分母にする。

`current_result_fouls` / `current_result`は実際の保存済み結果であり、
診断モデルで再計算したFoulや安打の件数ではない。診断からK率・打率は計算できない。
ファウル方向率とFoul結果率を混同しない。

## 診断モデルの仮定

座標はXが右、Yが前方、Zが上。
保存済みの実バット角度thetaと接触ずれx,zから、符号付き厚みを再構成する。

```text
t = -x*sin(theta) + z*cos(theta)
s = t / 0.070
n_local = (-sin(theta)*s, sqrt(1-s*s), cos(theta)*s)
```

半径0.070mと符号付き厚みの意味は現行モデルと共通。
保存済みの`signed_thickness_offset_m`とも比較するが、診断は再構成値を使用する。
半径を明確に超える入力は比較から除外する。

実際のattack angleは保存されていないため、現行のVLA式を逆算する。
選手マスターの現在値は使わず、その接触時の保存値を使う。

```text
attack = current_VLA - 0.60*degrees(asin(clamp(contact_offset_z_m/0.070)))
face = 0.85*(approach_variation_deg + pull_bias_deg + timing_angle_error)
bat_velocity_local = (0, bat_speed*cos(attack), bat_speed*sin(attack))
```

法線とバット速度をfaceだけ水平方向へ回転し、保存された実際の打席の符号でXを反転する。
左右打者の鏡映対称性を保つ。

**接触直前の投球速度ベクトルは保存されていない。**
`player_game_pitching.speed`（m/s）をスカラー速度の代理値とし、
`incoming_velocity=(0,-pitch_speed,0)`と仮定する。
これは投球の入射角、飛行中の減速、変化球の接触直前の速度を復元したものではない。

バットからボールへ向く単位法線nに対して、滑り摩擦なしの法線方向の衝撃を計算する。

```text
u = dot(incoming_velocity - bat_velocity, n)
k = (1 + restitution) / (1 + ball_mass/effective_bat_mass)
delta_v_normal = max(0, -k*u)
outgoing_velocity = incoming_velocity + delta_v_normal*n
```

ボール質量0.145kgも診断用の明示的な仮定。
離反中（u>=0）は衝撃を加えず、`separating_contacts`として数える。
この件数が多い場合は、復元した接触幾何と速度の前提が整合していない可能性があり、
Foul率の低いパラメーターだけを選んで採用しない。

HLA、VLA、EVはすべて同じoutgoing_velocityから求める。

```text
EV  = length(outgoing_velocity)
HLA = degrees(atan2(vx, vy))
VLA = degrees(atan2(vz, hypot(vx, vy)))
```

## 結果の限界

これは保存済み接触に対する近似の比較であり、完成した衝突物理モデルではない。
入射速度の仮定、接触法線の復元、バットの有効質量によって結果が変わる。
長さ方向の接触位置による有効質量・バット速度の変化、接線方向の摩擦・スピン、
球やバットの変形は扱わない。現行EVのずれによる減衰式も重ねない。

バットの上向き速度があっても、中心接触の法線が水平なら法線衝撃は上向き成分を持たない。
したがって中心接触のVLAは現行のattack angleと一致するとは限らない。
この差も接触幾何の仮定を評価する対象とする。

診断後の軌道、風・スピン、守備、打席継続は再計算しない。
極低EVの後方打球が前方へ動いた例23689/130についても、
この診断だけで風・回転の寄与を証明することはできない。
必要なら現行軌道への入力（スピンを含む）を別途記録し、風・回転を無効にした再計算へ進む。

比較の判断では、方向率と併せてEV/VLAの分布、離反中の件数、パラメーター感度を確認する。
診断の出力を見てから、幾何の修正・追加ログ・実際の試合への適用範囲を決める。

## テスト

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
python3 -m unittest discover -s tests/sql -p 'test_*.py'
```

法線衝撃の運動量・反発条件、静止無限質量バットでの理想反射、ガリレイ不変性、
離反接触、左右対称性、入力検証、同一分母、対象範囲、守備追加送球、
DBの不変性、空の範囲、出力上書き防止を検証する。
