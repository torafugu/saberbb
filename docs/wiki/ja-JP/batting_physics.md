# 打球の物理

数式では $\sigma(x)=\frac{1}{1+e^{-x}}$、$\operatorname{clamp}(x,a,b)=\min(\max(x,a),b)$ とする。三角関数の入力はラジアンで、度への変換には $180/\pi$ を掛ける。変数名は実装との対応を保っている。

実装: [batting_resolver.rs](../../../src/domain/resolver/batting_resolver.rs)。2026-11-06時点。

## 座標と適用範囲

![球場座標](../images/ja-JP/baseball_coordinate.png)

世界座標はXが横、Yが前方、Zが高さ。球場上の位置はホームからの距離と方向角でも表す。速度はm/s、時間は秒、角度は度、回転数はrpm。
通常試合は以下の経験式を使用する。3D有限バットの法線・接線衝撃量からEV/VLA/回転を一体計算する新モデルは診断用であり、通常打球にはまだ適用しない。

## 接触から初速（EV）

[接触判定](batting_logic.md)で得た長さ方向の絶対値をL、厚み方向をT、タイミング由来の位置ずれをqとする。

$$
\begin{aligned}
p &= \sigma(\mathrm{swing\_power}) \\
\mathrm{effective\_power} & =
\begin{cases}
p\left[1+0.05\,\operatorname{clamp}\!\left(\dfrac{|q|}{0.20},0,1\right)\right], & q<0, \\
p\left[1-0.15\,\operatorname{clamp}\!\left(\dfrac{q}{0.20},0,1\right)\right], & q\geq0,
\end{cases} \\
c_{\mathrm{swing}} &= 1.12+0.16\,\mathrm{effective\_power} \\
\mathrm{EV}_{\max} &= 0.18\,\mathrm{ball\_speed}+c_{\mathrm{swing}}\,\mathrm{swing\_speed}
\end{aligned}
$$

T≤0.020mなら $e_{\mathrm{thick}}=1$、T≥0.070mなら0。その間は次のcos²減衰を使う。

$$
\begin{aligned}
e_{\mathrm{thick}} &=
\begin{cases}
1, & T\leq0.020, \\
\cos^2\!\left(\dfrac{T-0.020}{0.050}\,\dfrac{\pi}{2}\right), & 0.020<T<0.070, \\
0, & T\geq0.070,
\end{cases} \\
e_{\mathrm{len}} &= \operatorname{clamp}\!\left(1-\left(\frac{L}{0.350}\right)^2,0,1\right) \\
\mathrm{EV} &= \mathrm{EV}_{\max}\,e_{\mathrm{thick}}\,e_{\mathrm{len}}
\end{aligned}
$$

厚み減衰に使うのはTでありLではない。ContactType別に固定のEVを抽選するモデルではない。

## 垂直打出し角（VLA）

$$
\mathrm{VLA}
= \mathrm{attack\_angle\_deg}
+0.60\,\frac{180}{\pi}\arcsin\!\left(
\operatorname{clamp}\!\left(\frac{\mathrm{offset\_z\_m}}{0.070},-1,1\right)
\right)
$$

ここで使うのは世界座標側のzずれ。符号付き厚み座標から一貫した3D衝突を解いているわけではない。EVが高くてもVLAが大きく負なら下向きの強い打球となる。

## 水平打出し角（HLA、version 2）

接触判定と同じprojectionを使い、バット傾斜thetaと符号付き厚みから反射方向を作る。

$$
\begin{aligned}
s &= \operatorname{clamp}\!\left(\frac{\mathrm{thickness\_signed}}{0.070},-1,1\right) \\
\mathbf{n} &= \left(-s\sin\theta,\;\sqrt{1-s^2},\;s\cos\theta\right) \\
\mathbf{r} &= (0,-1,0)+2n_y\mathbf{n} \\
\mathrm{rebound\_hla} &= \frac{180}{\pi}\operatorname{atan2}(r_x,r_y) \\
\mathrm{HLA} &= \operatorname{wrap}_{180^\circ}\!\left[
\mathrm{handedness}\left(0.85\,\mathrm{face\_angle\_deg}+\mathrm{rebound\_hla}\right)
\right]
\end{aligned}
$$

face angleは既存のapproach/pull抽選と元の時間差から求める。接触位置の0.75係数で元時間差を縮めず、Marginal専用のタイミング再抽選もしない。
HLAを±90度に丸めない。後方打球も符号付き前後速度を保持する。これは水平方向の理想反射近似であり、EV・VLA・回転を同じ保存則から導くモデルではない。[HLAモデル](../../contact_normal_hla.md)を参照。

## 回転

$$
\begin{aligned}
d &= \min\!\left(\sqrt{L^2+T^2},1\right) \\
\mathrm{collision\_rpm} &= 4000\,\frac{\mathrm{swing\_speed}}{33.333}\,d
\end{aligned}
$$

接触角から求めた衝突回転と投球回転の20%を、`SpinVector` の2成分ベクトルとして合成する。ここではL/Tの絶対値を使う。符号付き接触・入射角速度を使う完全な3D回転計算ではなく、EVと回転へのエネルギー配分も連成していない。
バックスピン・トップスピン・サイドスピンは力の向きを変えるため、「回転が多いほど必ず遠くへ飛ぶ」とは扱えない。低回転の不規則な揺れを生成する専用モデルもない。

## 軌道

初期位置は `(0,0,0.90)` m。速度を次のように分解し、0.01秒刻みで重力・空気抵抗・風・マグヌス効果・バウンド・フェンスを処理する。

$$
\begin{aligned}
\alpha &= \frac{\pi}{180}\,\mathrm{VLA}, & \beta &= \frac{\pi}{180}\,\mathrm{HLA} \\
v_x &= \mathrm{EV}\cos\alpha\sin\beta \\
v_y &= \mathrm{EV}\cos\alpha\cos\beta \\
v_z &= \mathrm{EV}\sin\alpha
\end{aligned}
$$

| 定数 | 値 |
| --- | ---: |
| AIR_RESISTANCE_COEFF | 0.0012 |
| MAGNUS_COEFF | 0.0000336 |
| RESTITUTION_COEFF（地面） | 0.45 |
| WALL_RESTITUTION | 0.60 |
| GROUND_FRICTION | 0.25 |

ファウル側の境界衝突（バックネットを含む）はFoulとして終了し、前方へ反射させない。フェア側でノーバウンドの柵越えはHomeRun、バウンド後の柵越えはGroundRuleDouble。Foul判定を先に行い、ファウル側の柵越えを二塁打にしない。
最初のバウンド・フェンス衝突の位置と時刻を保存する。[守備側の軌道推定](fielding_logic.md)はこの積分の再実行ではない。

## 診断用の有限バット衝突

[finite_bat_collision.rs](../../../src/domain/shared/finite_bat_collision.rs) は入射速度・回転、局所バット速度、有限質量・慣性、法線反発・摩擦・接線反発から出射速度と回転を計算する純粋な応答関数。
現行の接続は `finite_I0.00015_et0.4` に固定し、摩擦なし対照、エネルギー・衝撃量の検査、Rust/Python照合を記録する。端部・適用範囲外・未収束等を別状態として保持する。
通常の経験式を置き換えたと解釈せず、[診断とholdout](diagnostics.md)、[Rust応答仕様](../../rust_collision_response.md)から適用範囲と検証手順を確認する。
