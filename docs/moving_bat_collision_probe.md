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
確認済みの修正後範囲は23689～24048（28,502接触）。

```sh
python3 tools/diagnostics/contact_collision_probe.py --db /path/to/saberbb.db --first-game-id 23689 --last-game-id 24048 --output-dir ../saberbb_after_relative_probe
```

上記の試合IDは今回確認済みのafter範囲。別のデータセットでは対応する範囲へ変更する。
Windowsで`python3`がない場合は`python`に読み替える。
パスに空白がある場合は引用符で囲む。出力先は新しいディレクトリを指定し、
既存ディレクトリや前回の出力を上書きしない。
DBの必要列は現在のmainのスキーマを前提とする。

デフォルトは反発係数0.3 / 0.5 / 0.7と、有効バット質量0.3 / 0.6kgの6組。
固定断面の`moving_*`と相対運動に基づく`relative_*`それぞれに適用するため、診断は計12設定。
感度を見るための仮の値であり、現実の打球データで校正した値ではない。
任意の組み合わせに変えるには次のオプションを追加する。

```sh
--restitution 0.3 0.5 0.7 --effective-mass-kg 0.3 0.6
```

## 出力と読む順序

| ファイル | 内容 |
|---|---|
| coverage.csv | 指定範囲、観測された最小・最大試合ID、投球数、接触数、旧診断での比較可能数、全モデル共通の比較可能数、除外数、符号付き厚みの不一致数、probe_version=3 |
| direction_coverage.csv | 前回SQLの1番目。入力不足の接触も含む全接触の方向・結果件数 |
| direction_transitions.csv | 前回SQLの2番目。全接触の方向・結果・捕球方法の遷移表 |
| direction_gaps.csv | 前回SQLの3番目。方向と結果に差がある接触の全件集計 |
| excluded_inputs.csv | 衝突比較できなかった入力不足・異常値の理由別件数 |
| impact_parameter_summary.csv | 接触種別×打席符号×絶対b/R帯×b符号×variantの入力平均・EV/VLA・方向率 |
| impact_parameter_distribution.csv | 上記分類にEV帯・VLA帯・方向を加えた同時分布 |
| collision_summary.csv | 接触種別×現行／診断12設定の件数・平均EV/VLA・平均絶対HLA・ファウル方向率・後方方向率 |
| geometry_checks.csv | 法線の長さ・軸との直交・接近条件・相対運動による接触半径の誤差。係数設定による重複なし |
| collision_distribution.csv | 接触種別×モデル×EV帯×VLA帯×打ち出し方向の同時分布 |
| collision_contacts.csv | 同一接触ごとの入力、法線、バット速度、衝突後速度、HLA/VLA/EV。現行を含む13行／共通比較可能接触 |

最初に`coverage.csv`で対象範囲と接触数を確認する。前回と同じデータなら接触数は28,502。
投球数や接触数が一致しなければ、範囲や追加生成の有無を確認する。
方向遷移表の件数合計は入力接触数と一致し、追加送球による二重計上はない。
守備行なしの接触もLEFT JOINで保持する。

次に`direction_gaps.csv`で以下を確認する。

- ファウル側の最終方向なのにGroundRuleDoubleとなる接触が残っていないか。
- backward→fairが残る場合の接触種別・結果・件数。
- ファウルフライ捕球によるOutと、ゴロ処理のOut/DoublePlayを分ける。

最後に衝突比較を確認する。全モデルのcurrentと診断の分母は同じ比較可能接触で統一する。
入力不足や相対接触幾何を構成できない接触は全variantから除外し、`excluded_inputs.csv`に理由を残す。
`defined_hla_contacts` / `defined_vla_contacts`は角度が定義できる件数。
速度ゼロや鉛直方向では角度が未定義になり得るため、方向率は定義できる接触数を分母にする。

`current_result_fouls` / `current_result`は実際の保存済み結果であり、
診断モデルで再計算したFoulや安打の件数ではない。診断からK率・打率は計算できない。
ファウル方向率とFoul結果率を混同しない。

## 共通の入力と固定断面モデル（moving_*）

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

固定断面モデルでは法線とバット速度をfaceだけ水平方向へ回転し、保存された実際の打席の符号でXを反転する。
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

## b・R・b/Rの読み方

**bは、相対運動から求めたボール中心とバット中心軸との符号付き最接近距離。**
通常の距離としては`abs(b)`を使う。バット表面からの距離ではなく、中心軸からの距離。
**Rは接触可能な距離の上限で、バット半径＋ボール半径。**
現行の近似値は0.033m＋0.037m＝**0.070m（7cm）**。
バット半径だけを使うものではない。
`b/R`は無次元の比で、中心寄りか、かすり気味かを比較する。

| b | b/R | 円柱モデルでの意味 |
|---|---|---|
| 0cm | 0 | ボール中心の相対軌道がバット中心軸を通る |
| +3.5cm | +0.5 | 一方の側へ接触半径の半分だけずれる |
| -3.5cm | -0.5 | 反対側へ同じ距離だけずれる |
| ±6.3cm | ±0.9 | 表面をかする接触に近い |
| ±7cm | ±1 | 接線上でぎりぎり触れる |
| 絶対値7cm超 | 絶対値1超 | この近似では接触しない |

符号は`cross(バット軸, 接近方向)`の向きで決める。
バットの傾き・相対入射方向・打席符号に依存するため、単純にプラスが上、マイナスが下とは限らない。
バットの長さ方向の芯からのずれ（length offset）とは別の量。
軸方向に同じだけ位置を動かしても、無限長円柱への最接近距離bは変わらない。
また、従来の固定X/Z断面の`signed_thickness_offset_m`と、相対運動によるbは一般には一致しない。

個別CSVの対応は次のとおり。

| 意味 | 列 |
|---|---|
| 符号付きb（m） | impact_parameter_m。relative行に記載 |
| R（m） | collision_radius_m。全variantで0.070 |
| 符号付きb/R | b_over_r。共通の層別値として全variantに記載 |
| 絶対b/Rの帯 | b_abs_band |
| bの符号 | b_sign（negative / zero / positive） |

### b/R別集計

`impact_parameter_summary.csv`は、絶対値の帯を
`0..<0.2`、`0.2..<0.4`、`0.4..<0.6`、`0.6..<0.8`、`0.8..1`に分ける。
1ちょうどは最後の帯。境界の浮動小数点差を吸収するため、帯インデックスに1e-12の許容を置く。
各帯をbの符号・実打席符号・接触種別・variantで分け、上下や左右の相殺を確認できるようにする。

全variantを**relativeで求めた同じb/R**で層別する。
currentやmovingの独自のbを計算したという意味ではない。
各variantについて全帯・全符号・全接触種別を合算すると、共通比較接触数に一致する。
符号別の行を合算せず、currentとrelativeの率だけを直接平均しない。
率をまとめる場合は、それぞれの件数を使って加重する。

入力平均はバットのface回転前の`contact_offset_x_m/z_m`、固定断面の符号付き厚み、
再構成attack angle、保存されたtiming offset・timing impactを表示する。
これらは世界座標の実測位置ではない。
`avg_abs_fixed_radial`と`avg_abs_b_over_r`で、固定断面と相対運動による接近距離の違いを比較できる。
個別には`b_over_r`、集計では`avg_b_m`と`avg_abs_b_over_r`を使う。

タイミング入力がNULLの場合は欠損として、その平均の分母から除く。
`timing_offset_recorded_contacts` / `timing_impact_recorded_contacts`も出力する。
角度が未定義の接触は方向率・VLA率の分母に含めず、有効件数を残す。
`impact_parameter_distribution.csv`では同じ層別でEV/VLA/方向の同時分布を確認する。

基準時刻・座標系と本体の今後の接続方針は
[接触状態の座標・時刻の定義](contact_state_coordinates.md)を参照。

## 相対運動から作る接触法線（relative_*）

旧モデルと同じ接触データ・速度・係数を使い、接触法線だけを置き換える。
接触時のバット軸を`a_local=(cos(theta),0,sin(theta))`とし、
固定断面モデルと同じface回転・打席符号で世界座標へ変換する。
保存された位置ずれも`offset_local=(contact_offset_x_m,0,contact_offset_z_m)`から
同じ変換を行う。**奥行き方向のずれは未保存なのでY=0と仮定する。**

```text
u = incoming_velocity - bat_velocity
w = u - dot(u,a)*a                 # バット軸に垂直な相対速度
V = length(w)
d = -w/V                          # 接近する球を迎える方向
tangent = cross(a,d)
b = dot(offset,tangent)           # 符号付き最接近距離
q = b / R
n = sqrt(1-q*q)*d + q*tangent
```

これは一定の相対速度で、無限長の円柱へ最初に接触する側の法線。
`V=0`では構成不能、`abs(b)>R`では円柱への接触なしとして除外理由を出す。
どちらかの幾何で比較できない接触はcurrent・moving・relativeすべてから除外し、
同じ分母にそろえる。全接触の方向診断は除外しない。
旧モデルだけの比較可能数は`old_comparable_contacts`に残す。

最接近と最初の表面接触の時刻も相対的に求める。

```text
t_closest = -dot(offset,w) / (V*V)
t_entry = t_closest - sqrt(R*R-b*b)/V
```

`first_contact_time_offset_sec`は、仮定した保存位置ずれの基準に対する時刻差。
負値はその基準より前の接触を意味する。投球からの経過時間や実際の打撃タイミングではない。
バット軸方向の移動・有限のバット長・既存のlength miss判定の再計算は扱わない。
既に接触した球だけを対象とし、空振りを接触へ変更する反実仮想でもない。

この構成では`dot(u,n)=-V*sqrt(1-q*q)`となり、接近側であることは構成上保証される。
したがって、relativeの`separating_contacts`が減ること自体を、
元の入力や現実の衝突を正しく復元できた証拠にしない。
接触半径・軸との直交・法線長さを数値検証し、入力仮定の妥当性とは分けて扱う。

`geometry_checks.csv`のrelative行では次の値が数値誤差程度になることを確認する。

- `max_abs_normal_axis_dot`：法線とバット軸の内積の絶対値。
- `max_normal_length_error`：法線長さと1との差。
- `max_positive_relative_normal_ms`：離反側へ向く相対速度成分（正値のみ）。
- `max_contact_radius_error_m`：計算した最初の接触位置の半径とRとの差。

個別CSVには世界座標の軸・仮定した位置ずれ・最接近距離・横断速度・時刻差を残す。
current行の法線・接触時刻は未定義として空欄にし、旧幾何の値を現行の記録と混同させない。

VLA平均の正負相殺も確認できるよう、summaryに`avg_abs_vla_deg`、
`vla_lt0_pct`、`vla_10_25_pct`を追加した。EV/VLAの同時分布も引き続き出力する。
28,502接触がすべて比較可能なら個別CSVは370,526行（ヘッダー除く）。
集計評価にはまずcoverage、geometry_checks、collision_summaryを使う。

## 結果の限界

これは保存済み接触に対する近似の比較であり、完成した衝突物理モデルではない。
入射速度の仮定、接触法線の復元、バットの有効質量によって結果が変わる。
長さ方向の接触位置による有効質量・バット速度の変化、接線方向の摩擦・スピン、
球やバットの変形は扱わない。現行EVのずれによる減衰式も重ねない。

固定断面のmovingモデルでは、バットの上向き速度があっても、中心接触の法線が水平なら法線衝撃は上向き成分を持たない。
したがって中心接触のVLAは現行のattack angleと一致するとは限らない。
この差も接触幾何の仮定を評価する対象とする。relativeモデルでは相対入射方向に応じて中心接触の法線も傾く。
ただし、相対運動を正面入射・Yずれゼロ・直線運動で近似している限界は残る。

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
追加で、静止バットで旧法線へ一致する極限、上向きバット運動の中心接触、
軸方向平行移動への不変性、左右鏡映、接触半径・直交・単位長さ、退化・非接触・接線接触を検証する。

## 本体で保存した同一時計の状態を使う診断

従来の保存X/Zからの診断とは別に、本体で近似生成した3D状態を新しいテーブルへ保存できる。
こちらはDB更新と新規試合生成が必要。[接触状態の保存・診断](contact_state_shadow.md)を参照。
