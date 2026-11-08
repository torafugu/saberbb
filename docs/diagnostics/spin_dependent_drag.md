# Spin-dependent drag with unchanged lift

Normal gameplay now uses `a_drag = -K * CD * |v_air| * v_air`, with
`CD = 0.297 + 0.0292 * total_spin_rpm / 1000` and
`K = rho * pi * radius^2 / (2 * mass)`. Constants match the previously tested
reference: rho=1.194 kg/m3, mass=5.125 oz (0.145291306 kg), circumference=9.125 in,
radius=0.036888137 m. K has units 1/m; CD is dimensionless. The previous fixed
1/m factor was 0.0012; at 2600 rpm the new factor is 0.006550489.

Source: Alan M. Nathan, [Analysis of Baseball Trajectories](https://baseball.physics.illinois.edu/TrajectoryAnalysis.pdf),
2017-08-28, equations 7-10. This is a published fit to indoor 2016 MLB trajectories,
not an NPB or saberbb calibration. The original trajectory fit assumed negligible
gyro spin. Supplying measured/model total spin to its drag formula keeps total
and effective spin distinct; it is not independent validation of gyro-spin drag.

## Scope and spin inputs

Only the drag law changes. Lift coefficient/direction/cutoffs, collision,
Intent selection, random draws, wind, gravity, integration step, impact origin,
fences, bounce and rolling rules are unchanged. The existing live scalar spin
model assumes total spin equals its effective spin. The 3-D selected/frozen
observers instead pass total spin separately, including gyro, while their
transverse spin and angle continue to drive unchanged lift. The BattedBall spin
field still means the effective spin supplied to lift. No DB/schema changes.
Total and effective spin halve together on each ground bounce under the existing
bounce convention; no new in-air spin decay is introduced. A small 1e-8 rpm
comparison tolerance allows projection roundoff; nonfinite/negative spin and
materially total<effective inputs are rejected.

This does not activate 3-D collision in gameplay or repair ground-intersecting
contacts. Shadow flights still launch from the standardized .90 m origin. Those
are separate limitations from drag.

## Reproduce the paired comparison

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --example offense_season_check -- --season 2026 --games 360 --selected-intent-diagnostics --legacy-flight-drag > drag_legacy_360.json
cargo run --release --example offense_season_check -- --season 2026 --games 360 --selected-intent-diagnostics > drag_published_360.json
```

Default master seeds are 7,42,20261108; 360 games means 1080 total games per
command. Both runs read the same database and save no results/contact states.
Check matching `inputs_sha256`, season, games and seeds. JSON records the selected
drag model, constants and engine source hash. The historical model is selected
only within the synchronous diagnostic thread, restored after return/panic;
other threads use the published default. This override is not asynchronous
context propagation.

Compare full-game totals as seeded season outcomes, not identical play cohorts:
a changed out/hit changes subsequent pitches and RNG consumption. Changes in
walk/HBP counts do not imply changed pitching physics. Within each report,
`selected_intent_diagnostics.drag_comparison` replays identical accepted selected
collision outputs with both drag laws, fixing EV, VLA, HLA, effective/total/gyro
spin, height, wind and stadium. `attempted`, `paired`, `statuses` and outbound
`transitions` expose denominator/error coverage. Both groups share paired inputs;
non-foul memberships can still differ. Input is accepted selected flights under
the report's model, excluding unavailable collisions and primary flight errors.
These are shadow trajectory classifications, not actual game home-run totals.

The database-free `flight_audit` retains historical `live_discrete` and
`live_forces_rk4` labels as explicit old-force controls and adds
`published_drag_discrete`. `published_drag_live_lift` is the independent RK4
control for the new default. Other hybrid and full-reference models remain
available. The output schema advances to fixed_launch_flight_comparison_v2 and
selected_intent_forward_3d_v3.

## Development validation

Fixed launch: 45 m/s,20 degrees,2600 rpm,no wind,.90 m height carries 171.97 m
with old drag and 106.71 m with new drag; independent drag-only RK4 gives
107.06 m. All 36 default launch cases agree with the independent drag-only
control within .8 m carry and .05 s landing time. Those bounds test numerical
agreement, not real-world prediction accuracy. Historical full trajectory
fixtures still reproduce under the explicit legacy scope. Pure gyro spin with
zero effective spin increases drag without inventing lift; invalid spin inputs,
scoped restoration and thread isolation are tested.

Development roster, 12 games per seed (36 total), same input hash:

| Actual game metric | Legacy drag | Published drag |
|---|---:|---:|
| Plate appearances | 3309 | 3171 |
| At bats | 2961 | 2905 |
| Singles | 918 | 905 |
| Doubles | 20 | 45 |
| Home runs | 71 | 4 |
| Batting average | .3408 | .3284 |
| Runs | 686 | 464 |
| RA9 reference | 9.8208 | 6.4946 |

RA9 is runs*27/recorded outs, not ERA. This is a generated development roster,
not the user's 360-game dataset. The home-run reduction is substantial and could
be too large for the current scalar collision/lift combination. Batting average
and runs remain high; do not tune coefficients to this small sample or claim
league calibration. Obtain the real-roster paired reports before judging balance.

The three existing pitching-resolver tests for timing-offset sign and location
bias fail identically on unmodified main c48d01d; they are outside this change.
The other 499 library tests and all 22 offense example tests pass, including
full-GameResult recording parity. All 288 fixed-launch grid outputs land.
The source database hash remains unchanged. Results and paired shadow counts
are summarized in the PR.

On the new game's accepted selected-collision cohort, all 3477 inputs paired
successfully under both laws: shadow HR 768 -> 150, fair trajectories 2789 ->
2782, foul 688 -> 695. Launch/spin distributions for all paired inputs match
exactly. Of 768 legacy shadow HR, 150 remain HR, 498 become in-field, 108 become
ground-rule doubles and 12 become foul. The retained bounce model therefore
still merits separate inspection. These paired shadow values differ from
comparing old/new game-path cohorts (774 -> 150 shadow HR).

The legacy scope reproduces prior ordinary totals, team summaries and HR leaders
exactly. Adding the paired observer leaves new-game totals/team summaries/HR
leaders unchanged. Input hash, paired denominator, height partitions and outcome
transitions reconcile across the three seeds. The source database SHA256 stays
`519b8ca87c7f68126cd2e3b7e599401fc7c5d09fde40140d16573856d85b3a2e`.
