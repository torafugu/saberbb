# 同一時計の3D接触状態の保存・診断

## 今回の実装範囲

本体で各スイングのボール・バット状態を生成し、`player_game_contact_state`へ保存する。
接触だけでなく空振りも対象。見送りでは生成しない。
乱数を使わず、接触採否、ContactType、HLA/VLA/EV、軌道、試合結果を決める処理には渡さない。

**状態は近似モデルであり、実測値ではない。**
バージョン1、生成方法`plate_constant_accel_linear_bat_world_aim_proxy_v1`として保存する。
ボールは本塁面での一定加速度推定、バットは固定軸・一定速度の並進、認識誤差は狙い位置の代理値。
バットの回転や有限長の接触探索はまだ実装していない。

## DB更新と試合生成

新しいテーブルを作成してから、新コードで試合を追加生成する。旧データは保持する。
既存試合には状態を後付けしないため、23689～24048のデータだけでは新しい診断はできない。

リポジトリのルートで、SQLite CLIがある場合:

```sh
sqlite3 "/path/to/saberbb.db" ".read migrations/upgrades/add_contact_state_shadow.sql"
```

SQLite CLIがない場合は、上記SQLファイルをDBエディターで実行する。
CREATE TABLE IF NOT EXISTSのみの追加で、既存テーブル・試合は削除しない。
新規DBのDDLは`migrations/ddl/player_game_contact_state_table.sql`。
本体のDB更新が自動適用する仕組みを今回追加したわけではない。

テーブルのキーはgame_id/count_seq。state_version、state_source、invalid_reason、state_jsonを持つ。
有効な状態はJSONとして保存する。無効な入力はstate_jsonをNULLとし、理由を残す。
GameResultの新しいコレクションはserde(default)で旧JSONを読めるようにしてある。
状態は試合結果と同じトランザクションで保存する。

生成は打撃結果が確定する前に行うため、特殊な早期終了などで打撃結果行が残らなくても状態行を保持する。
診断ツールは状態・打撃結果のキーの和集合を使い、こうした球を`UnrecordedBatting`として残す。

## 基準時刻と座標

時計の0はリリース。t_refは保存されたflight_time、つまり本塁面Y=0の通過時刻の推定値。
世界座標Xは球場の右、Yは本塁から投手・外野方向、Zは上。
正規化されたBallLocationのyは高さであり、世界Yとは異なる。
標準ストライクゾーン寸法（半幅0.216m、半高さ0.325m、中心高さ0.825m）を使う。

ボールの位置は実際の正規化コースから世界X/Zへ変換する。
速度は既存`PitchedBall::velocity_at_plate()`で計算する。

```text
v_ball_ref = (target - release) / flight_time + 0.5 * acceleration * flight_time
```

一定加速度で終点を合わせる推定であり、抗力を逐次積分した接触直前の速度ではない。

## バット状態とタイミング誤差

認識・実行誤差のX/Zを、同時刻のボールとバット基準打点の差の代理値に使う。

```text
aim_error = (adapted_horizontal_offset + swing_error_x,
             0,
             adapted_vertical_offset + swing_error_z)
bat_ideal_position_ref = ball_position_ref - aim_error
v_bat = (0, bat_speed*cos(attack_angle), bat_speed*sin(attack_angle))
bat_position_ref = bat_ideal_position_ref - v_bat * timing_offset
r_ref = ball_position_ref - bat_position_ref
      = aim_error + v_bat * timing_offset
u_ref = v_ball_ref - v_bat
```

正のtiming_offsetは遅れ。同じ時計の理想バット軌道をtiming_offsetだけ遅らせる。
**旧timing_impact_x_mは新しい相対位置に加算しない。**
旧timing_position_scale=0.75も位相時刻には乗じない。入力のタイミング誤差を秒として使う。
旧X/Z・旧タイミング変位・旧scaleも比較用に保存する。

aim_errorのXは、上流の左右加速度差などに由来する世界Xの代理値として扱い、打席符号で再反転しない。
新状態のバット軸だけを`(handedness*cos(bat_angle),0,sin(bat_angle))`として作る。
角速度は0。水平面のface角は0とし、旧HLAのタイミングface角・乱数のapproach/pullを追加しない。
attack_angleはSwingContactResultの実行値を直接保存する。旧診断のようにVLAから逆算しない。

この狙い位置・軸・並進軌道の定義は移行用の仮定。
スイートスポット位置や完全な3Dスイングが既に解明されたという意味ではない。

## 保存する状態

state_json内の主な項目:

| 項目 | 内容 |
|---|---|
| input.reference_time_sec_since_release | リリースからt_refまでの時間 |
| input.ball_position_world_m / ball_velocity_world_ms | 同じt_refのボール中心位置・速度 |
| input.aim_error_world_m | 認識・実行誤差による狙い位置の代理値 |
| input.timing_offset_sec | 位相の遅れ。正が遅い |
| input.bat_speed_ms / attack_angle_deg / bat_angle_deg | 状態生成に使ったバット入力 |
| input.legacy_contact_offset_xz_m / legacy_timing_impact_x_m | 旧判定との比較用 |
| bat_ideal_position_world_m / bat_position_world_m | 理想・遅延後の基準打点位置 |
| bat_axis_world / bat_velocity_world_ms | 単位軸・同じ基準打点の速度 |
| bat_angular_velocity_world_rad_s | 今回は0 |
| relative_position_world_m / relative_velocity_world_ms | 同時刻の相対位置・速度 |

## 保存状態の診断

新しく生成した試合ID範囲で実行する。下の100～200は構文例。
出力先は存在しない新しいディレクトリにする。

```sh
python3 tools/diagnostics/contact_state_probe.py --db "/path/to/saberbb.db" --first-game-id 100 --last-game-id 200 --output-dir ../saberbb_contact_states
```

| 出力 | 内容 |
|---|---|
| state_coverage.csv | スイング数・記録済み・有効状態・打撃結果行なし・円柱交差件数 |
| state_status.csv | 旧ContactType別の未記録・無効入力・交差なし・円柱交差 |
| state_checks.csv | 位置・速度・位相・軸の独立再構成誤差、接触半径誤差、接触時刻差の最大絶対値 |
| state_collision_summary.csv | 有効な円柱交差についてcurrentとshadow6設定のEV/VLA・方向率 |
| state_collision_distribution.csv | 同じ対象のEV/VLA/方向分布 |
| state_contacts.csv | 旧X、新しい相対X/Y/Z、b/R、接触時刻、軸方向位置の個別比較 |

state_checksでは、保存されたrelative値をそのまま信頼せず、位置・速度・位相を入力から再計算する。
閉包誤差が1e-8を超える状態は診断対象から除外して理由を数える。
基準時刻差の最大値は誤差ではなく、線形近似を何秒延長したかを見るための値。
大きな時刻差を現実の接触時刻として無条件に解釈しない。

円柱は無限長。`within_legacy_axial_window`は、交差時の軸方向位置が旧判定の±0.16m内かを示す参考値。
バット端のキャップへの接触や有限バットへの初回接触を解いたものではなく、この値で交差を除外しない。
空振りや打撃結果行なしにはcurrentの打球EV/VLAを作らない。shadow設定だけを出力する。
接触も新状態が円柱と交差する部分集合で比較するため、coverageとstatusを先に確認する。
診断の交差件数から、実際のContact率や打率・K率を計算しない。

## 検証と限界

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
python3 -m unittest discover -s tests/sql -p 'test_*.py'
cargo test contact_state
```

Pythonでは保存状態の独立再計算、旧X変位を重ねないこと、DB移行の再実行、
NULL状態・旧データ・打撃結果行なし・空振りの分離、DB不変性を検証する。
Rustでは位相反映・左右鏡映・不正入力・DB往復・旧GameResult互換のテストを追加。
作業環境にRustツールチェーンがないため、Rustテストは未実行。
新しい試合を実行して、記録件数・誤差・分布を確認する必要がある。

本体の接触結果をこの近似で置き換える実装、スイングの回転軌道・有限長・摩擦・スピンは次段階。
元の試合結果との違いは診断として保持する。

## 旧miss原因とタイミング位相の比較

`contact_state_probe.py`は追加で次の2ファイルを出力する。
保存済み状態を読むだけなので、状態v1が保存されている24112～24471については、
DB更新・試合の再生成なしで実行できる。
出力先には以前と異なる、新しいディレクトリを指定する。

```sh
python3 tools/diagnostics/contact_state_probe.py --db "/path/to/saberbb.db" --first-game-id 24112 --last-game-id 24471 --output-dir ../saberbb_contact_state_miss
```

| 出力 | 内容 |
|---|---|
| state_miss_summary.csv | 旧ContactType × 再構成した旧miss原因 × 絶対タイミング帯ごとの交差率・b/R・時刻・軸位置・相関 |
| state_miss_contacts.csv | 有効状態1球1行。交差なしも残し、タイミング0との個別比較を出力 |

旧判定の再構成は、保存した旧X/Zと実行バット角θから行う。
旧接触判定に合わせ、ここでは打席符号を掛け直さない。

```text
length = old_x*cos(θ) + old_z*sin(θ)
thickness = -old_x*sin(θ) + old_z*cos(θ)
abs(length) > 0.16m       => length_miss
上記以外でabs(thickness) > 0.07m => thickness_miss
それ以外                  => contact
```

長さと厚みの両方が超過していても、現行コードの順序でlength_missとする。
境界ちょうどは超過ではない。contact内のSolid/Weak/Marginal分類や乱数は再実行しない。
`classification_mismatches`は旧幾何学判定と保存ContactTypeの接触／空振りが異なる件数。
最初にこの値が0であることを確認する。UnrecordedBattingは照合対象外（NULL）とする。

集計の`valid_states`は有効状態全体で、交差しなかった状態も分母に含む。
欠損・無効状態は既存state_status.csvに残す。
タイミング帯は絶対値の0..<2、2..<5、5..<10、10..<15、15..<25、25ms以上。
`timing_band=all`は同じグループの全帯域合計なので、各帯域と合算しない。

### 同じ球のタイミングだけ0にする比較

世界座標の速度・バット軸・狙い誤差はそのままに、相対位置を
`r = aim_error + v_bat*timing_offset`から`r0 = aim_error`に変える。
旧Xや旧ContactTypeは変更しない。ゲーム結果を再生する比較ではない。

| 主な列 | 定義・分母 |
|---|---|
| intersection_pct | 全有効状態に対する、実タイミングでの円柱交差率 |
| zero_timing_intersections | タイミングだけ0にした場合の交差件数 |
| gained_with_timing / lost_with_timing | 0では非交差→実タイミングで交差／その逆の件数 |
| avg_abs_b_over_r / avg_abs_delta_b_over_r | 横断速度が非ゼロの状態について、実タイミングの絶対b/R／0との差の絶対値の平均 |
| avg_first_entry_offset_ms | 交差した球の、基準時刻から初回進入までの符号付き時刻差 |
| avg_delta_first_entry_ms | 両条件で交差した球のみについて、実タイミングの初回進入時刻−0条件の時刻 |
| within_legacy_axial_window_pct | 実タイミングで交差した球のうち、交差時の軸方向位置が±0.16m内の割合 |
| timing_entry_pearson_r / timing_entry_slope_ms_per_ms | 実タイミングで交差した球について、符号付きタイミング誤差と初回進入時刻差のPearson相関／切片ありOLS傾き |
| timing_b_pearson_r / timing_b_slope_per_ms | 横断速度が非ゼロの状態について、符号付きタイミング誤差と符号付きb/Rの相関／OLS傾き |

時刻の平均・相関の分母は`cylinder_intersections`、bの分母は`geometry_states`、
両条件の時刻差の分母は`paired_intersections`。未定義の値は空欄にする。
相関は異なる球の投球・スイング条件も含むので、因果効果そのものではない。
同じ球の0条件との差と、次の解析的な感度を併せて読む。

### 直線移動モデルにおける解析的な感度

aを単位バット軸、uを相対速度、w=u−a(u·a)、V=|w|とする。
接触面の接線は`tangent = cross(a, -w/V)`。

```text
b(Δ) = aim_error·tangent + (v_bat·tangent)*Δ
closest_time(Δ) = -aim_error·w/V² - (v_bat·w/V²)*Δ
```

`db_dtiming_mm_per_ms`は各球の`v_bat·tangent`で、数値としてm/sとmm/msは同じ。
`closest_time_slope_ms_per_ms`は各球の`-v_bat·w/V²`。
これらと0条件を使った再構成誤差の最大値も集計に残す。
初回進入時刻はbに依存する平方根項を持つため、最接近時刻の傾きとは区別する。

例えばボール(0,-40,0)m/s、バット(0,40,0)m/s、軸(1,0,0)なら、
タイミングを20ms遅らせてもbは変わらず、最接近と初回進入が10ms遅れる。
固定軸の直線運動を無制限に延長すると、このように位相誤差が接触時刻へ吸収されうる。

円柱は引き続き無限長・時間窓制限なしであり、±0.16mは参考フラグ。
交差率をContact率に読み替えず、タイミング感度・軸位置・交差時刻を確認してから、
回転軌道や有効な探索区間の設計へ進む。係数調整や本体の採否変更はこの診断には含めない。

回転軌道との比較は[rotating_bat_probe.md](rotating_bat_probe.md)を参照。
同じ状態データから、基準打点の位置・速度を整合させた有限直線条件と3つの回転半径を比較する。
