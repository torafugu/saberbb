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
