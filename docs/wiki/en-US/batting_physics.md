# Batting Physics

In the equations, $\sigma(x)=\frac{1}{1+e^{-x}}$ and $\operatorname{clamp}(x,a,b)=\min(\max(x,a),b)$. Trigonometric inputs are radians; multiplying by $180/\pi$ converts to degrees. Variable names retain their implementation mapping.

Source: [batting_resolver.rs](../../../src/domain/resolver/batting_resolver.rs), as of 2026-11-06.

## Coordinates and scope

![Stadium coordinates](../images/en-US/baseball_coordinate.png)

World X is lateral, Y is forward and Z is height. Stadium locations also use distance and spray angle from home. Units are m/s, seconds, degrees and rpm.
Live games use the empirical formulas below. The newer 3D finite-bat model couples normal/tangential impulses, exit velocity and spin in diagnostics only.

## Exit velocity (EV)

Let L and T be absolute length/thickness offsets from [contact evaluation](batting_logic.md), and q the timing-derived displacement.

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

For T≤0.020m, $e_{\mathrm{thick}}=1$; for T≥0.070m, it is zero. Between those bounds:

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

Thickness decay uses T, not L. Exit speed is not drawn from a fixed distribution per ContactType.

## Vertical launch angle (VLA)

$$
\mathrm{VLA}
= \mathrm{attack\_angle\_deg}
+0.60\,\frac{180}{\pi}\arcsin\!\left(
\operatorname{clamp}\!\left(\frac{\mathrm{offset\_z\_m}}{0.070},-1,1\right)
\right)
$$

This uses world-frame vertical offset, rather than solving a single 3D collision from signed thickness. High EV with a strongly negative VLA still produces a hard downward ball.

## Horizontal launch angle (HLA, version 2)

Use the contact projection's signed thickness and actual tilt theta:

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

Face angle retains the existing approach/pull draws and original timing error. The 0.75 displacement scale does not shorten that error, and Marginal contact no longer resamples it.
HLA is not clamped to ±90 degrees; backward balls retain signed forward velocity. This is an idealized horizontal reflection approximation, not a common conservation-law solution for EV, VLA and spin. See [the HLA model](../../contact_normal_hla.md).

## Spin

$$
\begin{aligned}
d &= \min\!\left(\sqrt{L^2+T^2},1\right) \\
\mathrm{collision\_rpm} &= 4000\,\frac{\mathrm{swing\_speed}}{33.333}\,d
\end{aligned}
$$

Collision spin derived from contact angle is combined with 20% of pitch spin using the two-component `SpinVector`. This still uses absolute L/T, rather than a complete signed 3D contact and angular-velocity calculation. Energy allocation between EV and spin is not coupled.
Spin direction changes the force direction: more spin does not always increase distance. No dedicated low-spin flutter model is implemented.

## Trajectory

Start at `(0,0,0.90)` m and decompose velocity as follows. Integrate gravity, drag, wind, Magnus acceleration, bounces and fences in 0.01s steps.

$$
\begin{aligned}
\alpha &= \frac{\pi}{180}\,\mathrm{VLA}, & \beta &= \frac{\pi}{180}\,\mathrm{HLA} \\
v_x &= \mathrm{EV}\cos\alpha\sin\beta \\
v_y &= \mathrm{EV}\cos\alpha\cos\beta \\
v_z &= \mathrm{EV}\sin\alpha
\end{aligned}
$$

| Constant | Value |
| --- | ---: |
| AIR_RESISTANCE_COEFF | 0.0012 |
| MAGNUS_COEFF | 0.0000336 |
| RESTITUTION_COEFF (ground) | 0.45 |
| WALL_RESTITUTION | 0.60 |
| GROUND_FRICTION | 0.25 |

A foul-side boundary collision, including the backstop, terminates as Foul without reflecting forward. Fair fence clearance before a bounce is HomeRun; after a bounce it is GroundRuleDouble. Foul direction is checked first, preventing foul-side clearance from awarding a double.
First-bounce and fence-impact times and positions are recorded. [Fielding trajectory estimation](fielding_logic.md) does not rerun this integrator.

## Diagnostic finite-bat collision

[finite_bat_collision.rs](../../../src/domain/shared/finite_bat_collision.rs) is a pure response function using incoming velocity/spin, local bat velocity, finite mass/inertia, normal restitution, friction and tangential restitution.
The diagnostic connection freezes `finite_I0.00015_et0.4`, records a frictionless control and energy/impulse checks, and supports Rust/Python parity audits. Endcaps, out-of-range inputs and nonconvergence retain explicit statuses.
This has not replaced the live empirical formulas. See [diagnostics and holdouts](diagnostics.md) and [Rust response specification](../../rust_collision_response.md).
