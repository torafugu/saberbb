# 接触状態の座標・時刻の定義

本体の接触判定を3次元の衝突モデルへ接続するための設計整理。
現状の実装と、今後採用する状態の契約を分ける。
この文書によって本体の挙動やDB列を変更したわけではない。

## 現状の値は何を表すか

| 実装・保存値 | 現在の意味 | 3D接触状態としての制約 |
|---|---|---|
| PitchedBall.actual_location / BallLocation.x,y | ストライクゾーン基準で正規化された左右・高さ | ここでのyは世界座標の前後方向Yではない |
| release_point.x,y,z | 投球のリリース位置（m） | 世界座標。yは本塁より投手側が正 |
| PitchedBall.flight_time | リリースから本塁面までの推定飛行時間 | バットに接触する時刻を解いた値ではない |
| PitchedBall.velocity_at_plate() | リリース・本塁面の目標位置・一定加速度から推定した本塁面速度 | 関数は存在するが、現行接触診断では未使用。実際の衝突点速度を記録したものではない |
| PitchDisplacement.horizontal_offset_m / vertical_offset_m | 予測と実際の投球のずれに認識・補正を加えた接触判定入力 | ボールとバットの同時刻の世界座標差とは定義されていない |
| adapted_timing_offset_sec | 認識・補正後のタイミング誤差 | 現在の接触判定ではXずれ、打球角ではfaceの位相へ反映される |
| timing_impact_x_m | swing_speed × timing_offset × timing_position_scale | 接触判定ではXへ加算され、EV補正では前後の深さとして解釈される。世界X/Yへの一意な対応が未定義 |
| contact_offset_x_m / contact_offset_z_m | 上流のずれ・スイング誤差・タイミング変位を合わせた接触判定用X/Z | Y成分と基準時刻が保存されていない |
| signed_length_offset_m / signed_thickness_offset_m | 上のX/Zを実バット角度で回した局所成分 | 長さ方向と厚み方向の区別に使用。実際の円柱軌道との最接近を解いた値ではない |
| actual_bat_angle_deg | 接触判定のX/Z断面で使うバット角度 | 完全な3Dバット姿勢・角速度を表してはいない |
| launch_angle | 現行のattack angle＋ZずれによるVLA | 診断でattack angleを逆算する元。独立して観測されたスイング軌道ではない |

実装の参照先:

- `src/domain/resolver/pitching_resolver.rs`: calculate_flight_time、calculate_timing_offset、PitchDisplacement。
- `src/domain/resolver/batting_resolver.rs`: adapt_pitch_offset、evaluate_swing_contact_with_timing_scale、calculate_launch_angles、calculate_effective_c_swing。
- `src/domain/shared/ball.rs`: PitchedBall::velocity_at_plate。

現行の接触位置と角度は、判定モデル内の有効な値として保存されている。
これらに別名を付けてそのまま世界座標の物理状態として保存しても、基準時刻や奥行きが新たに分かるわけではない。

## 診断ツールの仮定

`contact_collision_probe.py`は以下を使う。

- 投球速度は`(0,-pitch_speed,0)`。
- 位置ずれは局所的に`(contact_offset_x_m,0,contact_offset_z_m)`。
- face回転と実打席の符号で軸・ずれ・バット速度を世界座標へ写す。
- attack angleは現行VLA式から逆算する。
- バット軸を固定した無限円柱と、一定の相対速度を仮定する。

したがって、診断の`first_contact_time_offset_sec`は、この仮の位置ずれを置いた基準からの時刻差。
`flight_time`や`catch_time_sec`とは異なり、そのままリリース時刻へ足せない。
接近側の法線と円柱表面の交点が得られても、この入力仮定の正しさまでは証明しない。

## 今後の接触状態の契約

### 世界座標

原点は本塁の地面上の基準点。Xは既存球場座標の右方向、Yは本塁から投手・外野へ向かう方向、Zは上。
位置はm、速度はm/s、角速度はrad/s、時間はs。
実際の右・左打席にかかわらず、世界座標の定義を変えない。
左右の対称化は局所スイングを世界へ変換するときに一度だけ行う。
正規化されたBallLocation.x,yは、ストライクゾーン寸法を使って世界X,Zへ変換する。

### 時計と基準時刻

- 共通時計の0はボールのリリース。
- 状態の比較基準t_refは、ボールが本塁面Y=0を通過する時刻。
- ボール状態とバット状態を**同じt_ref**で評価する。
- 実際の接触時刻t_contactは両者の幾何から別途求める。本塁面の通過時刻と一致するとは限らない。
- 相対接触時刻を保存する場合は、必ず基準時刻も保存し、t_contact=t_ref+dtと復元できるようにする。

### ボールとバットの状態

以下は今後の候補フィールドであり、現行DBに追加済みの列ではない。

| 候補 | 定義 |
|---|---|
| contact_state_version | 状態の定義・生成方法のバージョン |
| reference_time_sec_since_release | t_ref |
| ball_position_world_m | t_refにおけるボール中心位置3成分 |
| ball_velocity_world_ms | 同じt_refにおけるボール速度3成分 |
| bat_sweet_spot_world_m | 同じt_refにおけるバットの基準打点位置3成分 |
| bat_axis_world | バットの握り側から先端側へ向く単位ベクトル |
| bat_reference_velocity_world_ms | 同じt_refにおける基準打点の移動速度3成分 |
| bat_angular_velocity_world_rad_s | 同じt_refにおける角速度3成分 |
| state_source | 本体の軌道／終点からの推定／診断仮定などの由来 |

相対位置は`r=ball_position_world_m-bat_sweet_spot_world_m`とする。
バット表面の接触点の速度は、基準点速度に角速度による寄与を加える。

```text
v_bat_at_point = v_bat_reference + omega × (contact_point - bat_reference)
u_at_point = v_ball - v_bat_at_point
```

角速度と接触点の情報がない段階では、基準点の並進速度を使う近似だと明記する。
length方向の位置と、接触点のバット速度・有効質量との関係は、その後に接続する。

## 現行からの移行手順

1. **ボールの本塁面状態を既存モデルからそろえる。**
   本塁面位置は正規化コースを世界座標へ変換する。
   速度は既存のvelocity_at_plate()を候補にする。同関数は
   `v_ref=(target-release)/flight_time + 0.5*acceleration*flight_time`を返す。
   これは一定加速度による終点整合の推定であり、実測速度でも、抗力を逐次積分した速度でもない。
   由来と仮定を状態に付け、単なる`-pitch_speed`との違いを検証する。

2. **バットの軌道を、同じ時計で定義する。**
   理想の基準打点・軸・速度をt_refで評価できるようにする。
   タイミングの遅れを正と定義し、遅れDelta_tなら理想スイングの状態を
   `bat_state(t)=ideal_bat_state(t-Delta_t)`として評価する案を用いる。
   現在のtiming offsetの符号と、早い／遅いの例を照合してから接続する。
   さらに旧`speed*Delta_t`をXへ足すと二重反映になるため、両方式を重ねない。

3. **認識・実行誤差を、ボールの物理状態と分離する。**
   実際のボール軌道を認識誤差で動かすのではなく、狙う打点・スイング位相・バット姿勢へどう反映するかを定義する。
   旧offsetを移す際は、誤差の符号を例で検証する。
   `r=(old_x,0,old_z)`をそのまま実際の3D状態と呼ばない。

4. **接触探索と判定を、保存状態から再現できるようにする。**
   最初は短い区間で軸を固定した並進近似を診断に使い、適用区間と誤差を明記する。
   回転する有限長バットへの最初の接触、バット端の扱い、探索の時間範囲は次の段階で定義する。
   既存のContactTypeを保持して比較する診断と、新幾何による接触／空振りの再判定を混ぜない。

5. **試合への適用前に比較する。**
   同じ保存状態からHLA/VLA/EVと接触採否を再現できることを確認する。
   摩擦・スピン・有限長・有効質量などを未実装なら、その範囲を示して実データ分布と比較する。
   K率やFoul率だけを合わせて物理状態の整合性を判断しない。

## 接続時の検証項目

- 世界座標での左右鏡映と、局所から世界への変換往復。
- `r(t_ref+dt)=r(t_ref)+u*dt`が固定軸・一定速度の近似内で成立すること。
- 基準時刻をずらして同じ相対軌道を表現した場合、絶対接触時刻と接触位置が一致すること。
- 早い／遅いの符号、タイミング誤差ゼロ、速度ゼロ、接線接触、軸方向の移動。
- 保存状態からの独立再計算で、接触時刻・位置・法線・出射速度が一致すること。
- 入力不足や推定状態を、精密な状態として無言で補完しないこと。

固定軸・一定速度の診断関数については、基準時刻をずらしても絶対接触時刻・位置・法線が一致する回帰テストを追加した。

今回の改修で追加するのはb/R別の診断集計、この定義整理と診断関数の検証まで。
上記の本体状態生成・保存・有限長バット探索は、未実装の次段階として扱う。
