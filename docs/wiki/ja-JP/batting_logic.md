# 打撃ロジック

数式では $\sigma(x)=\frac{1}{1+e^{-x}}$、$\operatorname{clamp}(x,a,b)=\min(\max(x,a),b)$ とする。三角関数の入力はラジアンで、度への変換には $180/\pi$ を掛ける。変数名は実装との対応を保っている。

実装: [batting_resolver.rs](../../../src/domain/resolver/batting_resolver.rs)、[GameState::process_count](../../../src/domain/shared/game_state.rs)。2026-11-06時点の通常試合の処理。

## 投球とスイング判断

実投球は投手・捕手の[配球統合](pitch_call_decision.md)で決定する。予想投球は現在も別の `create_pitch` 呼び出しで生成する近似。両者の変化量・到達時間から `PitchDisplacement` を計算する。`calculate_location_bias` のタイミング・水平・垂直の追加バイアスは現在すべて0。

`calculate_batting_factor` は実コースと予想コース、球種の類似度、得意ゾーンから次の係数を作る。

$$
\mathrm{total\_modifier}
= \frac{(1-\mathrm{zone\_aptitude})+(1-\mathrm{zone\_similarity})+(1-\mathrm{pitch\_similarity})}{3}
$$

`calculate_swing_decision` のscoreは確率そのものではない。実コースのゾーン外距離を $d=\max(\mathrm{distance\_from\_zone\_edge},0)$ とし、以下を加算する。

| 成分 | 現在の処理 |
| --- | --- |
| count | `CountStatus::prob()` |
| approach | 通常は `PlateApproach::prob()`。2ストライクでは $\max(\mathrm{prob},0)$ |
| protection | 2ストライクのみ $1.75e^{-12d}$ |
| zone | $-2d$ |
| pitch | FourSeamFastballは+0.1、それ以外は−0.05 |
| anticipation | zone_similarity + pitch_similarity。2ストライクで負なら1/4 |
| aptitude | zone_aptitude |

$u<\sigma(\mathrm{score})$ ならSwing、それ以外はTake。ここで $u$ は一様乱数。判断用の乱数は1回。
現在の `default_plate_approach` の候補は全BatterTypeでAggressiveのみ。新しいBatterCharacterとは別の仕組み。
見送りは通常のボール・ストライク等の判定へ進む。詳細は[見送り診断](../../swing_decisions.md)。実コースを使う近似であり、独立した知覚コースからの判断ではない。

## 適応と実行誤差

`adapt_to_pitch` は次の係数を水平・垂直・時間のずれに掛ける。

$$
\mathrm{adaptation}
= \operatorname{clamp}\!\left(
\frac{1-\sigma(\mathrm{bat\_control})+\mathrm{total\_modifier}}{2},\;0.1,\;1.2
\right)
$$

bat_controlが高いほど残留誤差は小さくなる。batting_eyeによるタイミング知覚誤差も $1-\sigma(\mathrm{batting\_eye})$ で減衰する。
バット角度の基準は $\operatorname{clamp}(30-15\,\mathrm{location.y},10,60)$ 度。角度誤差と動的attack angleは引き続き計算するが、返却する `additional_x_m` / `additional_z_m` は現在0。角度誤差を並進誤差として二重加算しない。

## 接触判定

時間差を変えず、接触位置に換算する成分だけに0.75を掛ける。

$$
\begin{aligned}
\mathrm{timing\_impact\_x} &= 0.75\,\mathrm{swing\_speed}\,\mathrm{timing\_offset\_sec} \\
x &= \mathrm{horizontal\_offset}+\mathrm{additional\_x}+\mathrm{timing\_impact\_x} \\
z &= \mathrm{vertical\_offset}+\mathrm{additional\_z} \\
\mathrm{length\_signed} &= x\cos\theta+z\sin\theta \\
\mathrm{thickness\_signed} &= -x\sin\theta+z\cos\theta
\end{aligned}
$$

thetaは実バット傾斜角。絶対値で接触範囲を判定し、符号付き座標も診断へ保存する。

| 判定順 | 条件 |
| --- | --- |
| SwungAndMiss | 長さ方向 > 0.350m、または厚み方向 > 0.070m |
| MarginalContact | 厚み方向 > 0.055m |
| WeakContact | 厚み方向 > 0.025m |
| SolidContact | 上記以外 |

さらに先端寄りの接触は既存の確率的品質低下を受ける。MarginalContactはファウル確定を意味しない。
長さ・厚みのmissは独立した診断フラグで確認する。[接触miss診断](../../contact_miss_diagnostics.md)、[75%位置ずれ](../../timing_position_trial.md)を参照。

## 打球と診断経路

通常の打球は[打球物理](batting_physics.md)のEV・VLA・HLA・スピンから軌道を生成する。Marginal専用のタイミング再抽選は行わない。
別途、Playerの性格と監督からIntentを決め、予測→計画→実行→接触→有限バット衝突応答を診断として記録する。
この応答は通常のスイング／見送り・接触・得点を置換しない。[打撃戦略と監督](batting_strategy.md)、[診断とholdout](diagnostics.md)を参照。
