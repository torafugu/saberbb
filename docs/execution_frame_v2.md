# 共通支点によるスイング実行誤差の診断入力v2

## 今回の変更

理想角から実行角へ傾きを変える際、同じ支点から打点位置・軸・速度を一体で生成する。
従来の「実行軸に変えてから基準打点を狙い位置へ配置する」条件と、同じ球・同じ半径で比較する。
本体Rustの打撃判定・乱数・DB保存内容は変更しない。

**v2は診断側で生成する入力モデルのバージョン。DBの保存状態はv1のまま。**
新しい試合やDB移行は不要。実行時にJSONLへ生成するので、入力の定義を確認・再利用できる。
旧DB行へ新しい状態を後付けする処理ではない。

```sh
python3 tools/diagnostics/execution_frame_probe.py --db "/path/to/saberbb.db" --first-game-id 24112 --last-game-id 24471 --output-dir ../saberbb_execution_frame_v2
```

Python標準ライブラリのみを使い、DBは読み取り専用のスナップショットで参照する。
出力先は未作成のディレクトリを指定する。
既定では3半径×3条件で1球9行、入力JSONLは1球×半径で3行。
集計はストリーミングで行い、全明細をメモリに保持しない。

## 対応する保存入力と理想角

対応する保存sourceは`plate_constant_accel_linear_bat_world_aim_proxy_v1`、state_version=1。
既存ツールと同じキーの和集合・入力検証を使い、欠損・無効・未対応状態を明示する。
UnrecordedBattingも独立した比較ラベルとして残す。

保存された世界Zは標準ストライクゾーンから生成されているので、次の現行コードと同じ規則で理想角を再構成する。

```text
normalized_y = (saved_ball_z - 0.825)/0.325
ideal_angle = clamp(30 - 15*normalized_y, 10, 60) degrees
execution_error = saved_actual_bat_angle - ideal_angle
```

これは`batting_resolver.rs::calculate_bat_angle()`と、保存source v1のゾーン寸法に対応する。
理想角が歴史的な実測値として保存されていたという意味ではない。
高さ→理想角の規則や保存座標を変更した場合は、source/versionごとに別の変換が必要。

現行`calculate_swing_execution_error()`は追加X/Zを計算しているが返却値を0にしている。
今回、それを本体へ戻す修正はしない。角度誤差を共通支点の回転として一度だけ適用し、
位置差を同じ幾何学から求める。元の追加Z=L sin(delta)は理想軸を水平とした式であり、
世界座標に傾いた軸の回転と同じ位置差とは限らない。
角度誤差を増幅したり、新しい乱数を追加したりもしない。

## 位置・軸・速度の定義

世界X/Y/Zとリリース時計は[3D接触仕様](contact_3d_contract.md)に従う。
保存された実行速度v1と実行軸a1を保持し、打席符号hを使って理想軸a0を定義する。

```text
a0 = (h*cos(ideal_angle), 0, sin(ideal_angle))
a1 = saved_actual_axis
R = world-Y rotation by -h*execution_error
p0 = saved_ball_position - saved_aim_error
P0 = p0 - L*a0
```

R a0=a1。世界Xの狙い誤差を打席符号で再反転しない。
P0が理想・実行で共有する支点。狙い・実行位置の代理誤差はp0の配置に一度だけ使う。
保存source v1では追加の実行X/Zが0であることを前提とするため、将来その意味を変える場合はsourceも区別する。

### 共有する支点速度と軌道速度

基準時刻で実行速度を保存値v1に合わせるため、支点速度を
`vP=a1*dot(v1,a1)`とし、残りを軌道速度に分ける。

```text
orbital_actual = v1 - vP
orbital_ideal = inverse(R)*orbital_actual
omega0 = cross(a0, orbital_ideal)/L
omega1 = R*omega0

v_ideal_ref = vP + cross(omega0, L*a0)
v_actual_ref = vP + cross(omega1, L*a1) = v1
```

**理想速度は、保存された実行速度から逆回転して作る反実仮想の値。**
打者が意図した速度を独立に復元・計測した値ではない。
保存されたactual attack angleを理想攻撃角へ逆算して別に加算する処理はしない。
この定義により、実行速度・実行軸を従来条件と同じにし、支点を共有した位置差の影響を比較できる。
支点速度は両条件で同じ世界ベクトルで、理想軸に平行とは限らない。

### タイミングを含む軌道

```text
s = t - timing_offset
P(s) = P0 + vP*s
ideal_axis(s) = Rodrigues(omega0*s)*a0
actual_axis(s) = Rodrigues(omega1*s)*a1 = R*ideal_axis(s)
ideal_point(s) = P(s) + L*ideal_axis(s)
actual_point(s) = P(s) + L*actual_axis(s)
```

支点位置・速度は同じ時計で共有する。位置差・軸・支点からの相対速度が同じRで変換されることを検証する。
角度誤差0なら、従来条件・理想条件・実行条件の軌道が一致する。
基準時刻の理想点と実行点の距離は`2*L*abs(sin(execution_error/2))`。
旧timing X、旧HLA timing角、旧VLAへのattack角加算は使わない。

## 比較する3条件

| frame_model | 条件 |
|---|---|
| reanchored | 従来の回転診断。実行軸・実行速度を使い、基準打点をp0へ再配置する |
| ideal_no_execution | 共通支点・理想軸・逆回転した理想速度。角度誤差を適用しない |
| shared_pivot_execution | 共通支点を保持し、Rで位置・軸・支点からの相対速度を変える |

3条件とも同じタイミング誤差で動かし、それぞれの条件内でタイミング0とも比較する。
理想条件の名称は角度誤差なしを意味し、狙い誤差やタイミング誤差まで0にする意味ではない。

reanchoredとshared_pivot_executionは実行軸・角速度・支点速度が一致し、軌道間に一定の位置差がある。
その比較が、実行角を変えた後の打点再配置によって失われていた位置差の効果を切り分ける。
ideal_no_executionとshared_pivot_executionは支点を共有し、角度誤差全体の効果を比較する。

接触区間±0.16m、有効半径0.07m、時間窓±60ms、e=0.5、M=0.6kgを既定とし、
半径0.6/0.9/1.2mを比較する。探索開始はリリース時刻で切る。
形状・探索・衝突式・品質集計は既存のMotion/searchと共通3D接触処理を再利用する。
反復上限4,000、空間許容差1µm。未収束・開始時近接は別の状態として保持する。
`--levers-m`、`--half-window-ms`、`--half-length-m`、`--contact-tolerance-m`、`--max-iterations`で変更できる。

## 出力と確認順

| ファイル | 内容 |
|---|---|
| execution_frame_coverage.csv | 対象キー・有効状態・入力JSONL行数・試行行数 |
| execution_frame_status.csv | 欠損・無効・未対応・有効の状態数 |
| execution_frame_checks.csv | 共通支点、基準速度、時刻を進めた位置・軸・速度の回転関係の最大残差 |
| execution_frame_summary.csv | 旧分類×miss原因×タイミング帯×条件の接触・EV/VLA・法線速度比・EV40以上率 |
| execution_frame_angle_bands.csv | 実行角度誤差帯別の接触・非接触・未解決件数 |
| execution_frame_transitions.csv | 同じ球のreanchored→共通支点、および理想→実行の状態遷移 |
| execution_frame_pairs.csv | 両条件で力積正となった同一球のEV/HLA/VLA・法線速度比比較 |
| execution_frame_contacts.csv | 各球・条件の接触状態、位置差、速度差、角度誤差と既存の接触明細 |
| execution_frame_inputs.jsonl | 各球・半径の診断入力v2。支点、理想／実行の位置・軸・速度・角速度 |
| execution_frame_metadata.json | 入力source、導出規則、条件、係数、探索設定 |
| contact_3d_status.csv / contact_3d_quality.csv | 共通の接触状態・品質記述別集計 |

最初にcoverage/status/checksを確認する。入力変換の残差が1e-8を超える球は理由を残して除外し、
全半径・全条件を同じ集合から出力する。
`timing_band=all`は全帯域合計。帯域別行と合算しない。
summaryは条件ごとの接触集合、pairsは同じ球で両条件ともimpulse_contactになった集合。
HLA差は±180°へ折り返してから絶対値を取る。角度未定義の場合はその指標の比較から外して件数を記録する。
探索statusのcontactと、正の力積を受けるimpulse_contactは区別する。
角度誤差帯は絶対値の0.1°未満、0.1..<0.5、0.5..<1、1..<2、2°以上。

明細の`reference_position_shift_m`は保存済みp0から各条件の基準時刻・タイミング0の打点までの距離。
`reference_velocity_vs_saved_ms`は保存速度との差。実行条件と従来条件では数値精度内で0になる。
位相適用後の差は`phase_position_difference_m`などで別に記録される。
`linear_infinite_*`は旧保存フレームの参考なのでreanchored以外ではnot_applicableとする。
本CLIは独自の入力v2メタデータを持つ。品質集計は実行時に出力するため、rotating専用のCSV監査CLIへ渡す必要はない。

## 解釈と限界

角度誤差が元々小さければ、打点位置差も小さく、接触数・品質の変化が小さい可能性がある。
その結果もそのまま報告し、誤差の倍率や衝突係数をContact率に合わせて追加しない。
支点位置、L、支点速度、理想速度は比較用のモデル定義。実際の手・身体の位置を復元したものではない。
特に理想速度は保存された実行速度に条件付けられており、意図・実行の因果モデルを同定したものではない。

この変更は「追加X/Zを0で返した実装が誤りだった」と確定するものではない。
位置と姿勢を一つの支点で結びつける定義を明示し、打点の再配置による差を計測する。
本体適用には、打者能力・コース認識・角度実行誤差の分布を別途検証する必要がある。

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
python3 -m unittest discover -s tests/sql -p 'test_*.py'
```

左右鏡映、角度誤差0、解析的な位置差、基準速度、軌道の数値微分、位相の一回適用、
一定の対照条件との位置差、旧比較値の隔離、ストリーミング集計の既存集計との一致、
DB不変性・欠損・未対応source・孤立状態・空範囲・重複半径をテストする。
