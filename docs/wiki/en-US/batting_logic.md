# Batting Logic

In the equations, $\sigma(x)=\frac{1}{1+e^{-x}}$ and $\operatorname{clamp}(x,a,b)=\min(\max(x,a),b)$. Trigonometric inputs are radians; multiplying by $180/\pi$ converts to degrees. Variable names retain their implementation mapping.

Sources: [batting_resolver.rs](../../../src/domain/resolver/batting_resolver.rs) and [GameState::process_count](../../../src/domain/shared/game_state.rs). Live behavior as of 2026-11-06.

## Pitch and swing decision

The actual pitch comes from [pitcher/catcher reconciliation](pitch_call_decision.md). The expected pitch is still sampled separately through `create_pitch` as an approximation. Movement and flight-time differences produce `PitchDisplacement`. All additional timing, horizontal and vertical terms in `calculate_location_bias` are currently zero.

`calculate_batting_factor` evaluates actual/expected locations, pitch similarity and zone aptitude:

$$
\mathrm{total\_modifier}
= \frac{(1-\mathrm{zone\_aptitude})+(1-\mathrm{zone\_similarity})+(1-\mathrm{pitch\_similarity})}{3}
$$

The score from `calculate_swing_decision` is a logit. Let $d=\max(\mathrm{distance\_from\_zone\_edge},0)$ using the actual location.

| Component | Current behavior |
| --- | --- |
| count | `CountStatus::prob()` |
| approach | `PlateApproach::prob()`; with two strikes, $\max(\mathrm{prob},0)$ |
| protection | With two strikes only, $1.75e^{-12d}$ |
| zone | $-2d$ |
| pitch | +0.1 for FourSeamFastball, −0.05 otherwise |
| anticipation | zone_similarity + pitch_similarity; negative values are quartered with two strikes |
| aptitude | zone_aptitude |

Swing when $u<\sigma(\mathrm{score})$; otherwise take. Here $u$ is a uniform random draw. This consumes one decision draw. Currently every BatterType's `default_plate_approach` contains only Aggressive, separately from the new BatterCharacter. A take enters ordinary pitch adjudication. This decision still uses the actual location rather than an independent perceived location. See [swing decision diagnostics](../../swing_decisions.md).

## Adaptation and execution error

`adapt_to_pitch` multiplies horizontal, vertical and timing errors by:

$$
\mathrm{adaptation}
= \operatorname{clamp}\!\left(
\frac{1-\sigma(\mathrm{bat\_control})+\mathrm{total\_modifier}}{2},\;0.1,\;1.2
\right)
$$

Higher bat_control reduces residual error. Timing perception also uses $1-\sigma(\mathrm{batting\_eye})$ to reduce error as batting_eye increases.
Nominal bat tilt is $\operatorname{clamp}(30-15\,\mathrm{location.y},10,60)$ degrees. Angular error and dynamic attack angle remain active, but returned `additional_x_m` / `additional_z_m` are currently zero; angular error is not also added as translation.

## Contact

Only timing-derived displacement is scaled by 0.75; the original timing offset remains unchanged.

$$
\begin{aligned}
\mathrm{timing\_impact\_x} &= 0.75\,\mathrm{swing\_speed}\,\mathrm{timing\_offset\_sec} \\
x &= \mathrm{horizontal\_offset}+\mathrm{additional\_x}+\mathrm{timing\_impact\_x} \\
z &= \mathrm{vertical\_offset}+\mathrm{additional\_z} \\
\mathrm{length\_signed} &= x\cos\theta+z\sin\theta \\
\mathrm{thickness\_signed} &= -x\sin\theta+z\cos\theta
\end{aligned}
$$

theta is the actual bat tilt. Contact checks use absolute values; diagnostics preserve signed coordinates too.

| Check order | Condition |
| --- | --- |
| SwungAndMiss | Length > 0.350m or thickness > 0.070m |
| MarginalContact | Thickness > 0.055m |
| WeakContact | Thickness > 0.025m |
| SolidContact | Otherwise |

Existing probabilistic tip penalties can lower contact quality further. MarginalContact does not guarantee a foul. Length/thickness misses have independent diagnostic flags. See [miss diagnostics](../../contact_miss_diagnostics.md) and [75% timing displacement](../../timing_position_trial.md).

## Batted ball and diagnostic path

Live contact uses the EV, VLA, HLA and spin rules in [batting physics](batting_physics.md). Marginal contact no longer resamples timing.
A separate path derives Intent from the Player and Manager and records forecast, plan, execution, contact and finite-bat collision response diagnostics. That response does not replace live swing/take decisions, contact or scoring. See [batting strategy and managers](batting_strategy.md) and [diagnostics and holdouts](diagnostics.md).
