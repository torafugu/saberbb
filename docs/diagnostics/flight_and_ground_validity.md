# Fixed-launch flight and contact-ground validity audit

## Commands

The fixed-launch comparison needs no database:

```sh
cargo run --release --example flight_audit > flight_audit.json
```

On a real roster, reuse the selected-Intent audit with its new height fields:

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --example offense_season_check -- --season 2026 --games 360 --selected-intent-diagnostics > selected_intent_validity_360.json
```

The original audit added diagnostics only. The follow-up [drag change](spin_dependent_drag.md)
adopts published drag as the default while retaining the original .90 m height,
stadium wind, .01 s update, lift, fences, bounces and rolling. Raw shadow states remain per-game in memory; no DB
or saved result/contact-state writes are introduced.

## Measurement-based reference and numerical validation

Reference: Alan M. Nathan, [Analysis of Baseball Trajectories](https://baseball.physics.illinois.edu/TrajectoryAnalysis.pdf),
2017-08-28, equations 7, 10-11. Its force coefficients were fitted to 2016
Tropicana Field Statcast trajectories. It is a published measurement-fitted
model, **not a set of individual measured endpoints** supplied to this audit.

The reference uses CD=.297+.0292*(rpm/1000) and
CL=1.120*S/(.583+2.333*S), where S=R*omega/v. SI parameters here use the nominal
5.125 oz mass, 9.125 inch circumference and indoor density 1.194 kg/m3. Both
calculations use the engine's g=9.81 m/s2. The reference holds world spin fixed;
an additional tau=30 s exponential-decay run is a sensitivity, not new data.

This is not validation against private Trackman records or NPB balls. The
conservative EV/VLA comparison window is 90-110 mph and 20-35 degrees; it does
not establish a measured validity bound for every supplied spin value. No new
saberbb coefficient is fitted by this command.

The independent diagnostic solver is RK4 with an interpolated first-ground event.
A separate SciPy DOP853 implementation generates nine numerical golden cases
(including wind, hybrids, decay and vacuum). Vacuum range/time are also checked
against an analytic solution; timestep halving checks convergence. Numerical
agreement is not a statement of real-world prediction accuracy.

Regenerate the optional development oracle with SciPy installed:

```sh
python tools/diagnostics/generate_flight_reference_contract.py
```

## What the grid controls

Defaults: speeds 40.2336/45/49.1744 m/s, VLA 20/30/35 degrees, spin 0/400/2600/4000
rpm, straight-ahead backspin, height .90 m, no wind. All 36 inputs share the same
environment across eight outputs:

| Output | Purpose |
|---|---|
| live_discrete | Historical drag in the current implementation with explicit environment, stopping at first ground in a 500 m field |
| published_drag_discrete | Default published drag with existing lift in the current implementation |
| live_forces_rk4 | Historical force laws in the independent integrator: isolates update/event numerics |
| published2017 | Published drag/lift laws, constant world spin |
| published_drag_live_lift | Change drag only in the common solver |
| live_drag_published_lift | Change lift only in the common solver |
| published2017_tau30_sensitivity | Add decay to the reference model |
| vacuum_rk4 | Gravity-only control |

Live retains its 20-second time limit; the independent solver uses 30 seconds.
Carry is the first landing distance, not bounce/roll distance. Ground time, apex
height and an optional y=120 m crossing height are reported. The 120 m/2 m marker
is a diagnostic plane, not a stadium home-run decision. Timeouts and boundary
exits remain explicit; they never become zero carry.

Custom grids/environment use `--speeds-ms`, `--vla-deg`, `--spin-rpm`, `--height-m`,
`--wind-speed-ms`, `--wind-direction-deg`, `--dt-sec`. `still_air_force_comparison`
normalizes launch-speed scalars to equivalent CD/CL; integrations use relative
air velocity if wind is supplied. Mass/radius/gravity/settings are included in
JSON. The live scalar spin angle follows the air-velocity basis, while the
reference world spin axis is fixed. Default no-wind planar backspin avoids that
extra convention difference in the attribution controls.

## Ground validity of selected 3-D contacts

`selected_intent_forward_3d_v3` retains the original aggregate outcomes and adds:

- `contact_height_statuses` per Intent/cohort: center below ground (z<0), sphere
  intersects ground (0<=z<ball radius), clear of ground (z>=ball radius), and
  no geometry. The radius is read from the current finite model (.036 m).
- `reference_height_statuses`, `reference_z_m` and `height_transitions` from the
  plate-reference position to the searched contact position. These distinguish
  an already-low reference from a crossing during the contact-time offset. A
  below-ground reference alone does not invalidate an earlier above-ground hit.
- `ground_valid_outcomes` and `ground_invalid_outcomes`, partitioning accepted
  flights while keeping original `outcomes` unchanged. This tests instantaneous
  ball/ground clearance, not full-pitch or bat-body admissibility.
- `by_intent_and_height_validity` and dedicated bounded `invalid_ground_examples`
  with reference position/velocity, contact point, selected search and evaluation.

Nothing is clamped, reclassified as a miss or removed from saved game results.
Existing diagnostic flights still start at the standardized .90 m point. The new
clearance cohort does not silently substitute actual impact height. An upstream
pitch/ground or execution constraint can be assessed separately using these traces.

## Historical fixed-launch findings (before adopting drag)

All 252 default grid outputs landed. Example: 45 m/s, VLA 20 degrees, 2600 rpm,
no wind, .90 m height. Distances are computed model results, not observed balls:

| Calculation | Carry (m) |
|---|---:|
| Live .01 s first-ground | 171.97 |
| Live forces, RK4 | 172.11 |
| Reference drag, live lift | 107.06 |
| Live drag, reference lift | 235.90 |
| Reference drag and lift | 120.55 |
| Reference with tau=30 s | 120.30 |

At these nominal conditions, live drag is equivalent to CD=.0683 versus the
reference's .3729: live deceleration is 2.43 m/s2 versus 13.26 m/s2. Live lift is
3.93 m/s2 versus 8.06 m/s2. Thus, within this reference comparison, insufficient
drag is the main carry discrepancy; reducing Magnus alone would mask it. The
reference's stronger lift partly restores carry after increasing drag.

Without spin, the same 45 m/s,20-degree case carries 121.88 m live versus 94.34 m
with reference drag. The maximum discrete-vs-RK4 live-force carry difference
across the grid was .60 m, much smaller than the force-law differences here.
These historical controls motivated the subsequent drag-only change; the new discrete
default carries 106.71 m for the 45 m/s,20-degree,2600-rpm case. Calibration of
the combined collision/lift/fielding pipeline remains separate.

## Original audit validation and coverage

- Twenty-five batting-resolver tests (including six pre-refactor trajectory
  snapshots and explicit first-ground/environment cases), three reference-solver
  tests (nine independent DOP853 fixtures, vacuum analytic check, step convergence
  and invalid inputs), the existing full-GameResult recording-parity test (two
  seeds), and twenty-one example tests passed.
- The 36-input/seven-output grid produced 252 landing results with no errors.
- Twelve games x three seeds on the development roster preserved ordinary
  total/team/HR-leader payloads and all original selected-Intent matrices,
  statuses, outcomes and paired groups exactly. Height counts/transitions and
  valid/invalid accepted-flight groups reconciled. This small fixture had no
  ground-intersecting searched contacts; synthetic negative-height and radius
  boundary tests verify those branches and dedicated samples. Real-roster
  frequencies require the new report, not inference from a minimum height.
- The source database SHA256 stayed
  `519b8ca87c7f68126cd2e3b7e599401fc7c5d09fde40140d16573856d85b3a2e`.
  Scoped formatting and git diff --check passed. The existing unformatted
  resolver is not globally reformatted; only the new environment/test sections
  were formatted, preserving unrelated source.
