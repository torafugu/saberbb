# Moving-bat collision comparison on frozen legacy contacts

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --example offense_season_check -- --season 2026 --games 360 --moving-collision-diagnostics > moving_collision_360.json
```

The observer adds `moving_collision_diagnostics` per seed. It reads SABERBB_DB
without writing results/contact states, observes completed games and draws no RNG.
The live swing/contact/ball/fielding/running logic is unchanged. Other diagnostic
flags can be combined, but they are not required by this one.

## What is frozen, and what is assumed

The previous audit isolated a large contribution from the legacy cylindrical
normal-reflection term to weak-contact foul selection. This comparison reuses the
existing finite-mass/inertia collision solver, rather than zeroing the term or
replacing it with a fitted fair-ball rate.

Every live Solid/Weak/Marginal contact remains in the source cohort, including
fouls/HR. First reconstruct the recorded live HLA and flight endpoint, time,
height and outbound category. A mismatch aborts the report. Missing fields have
explicit coverage. Join the original pitch by both count_seq and pitcher_id,
requiring one record. No new contact search, Intent choice or swing execution
is substituted for the recorded legacy contact.

The legacy records do **not** contain a measured 3-D contact pose. The adapter is
therefore a documented pose proxy, not an exact physical reconstruction:

- Project recorded X/Z offsets through the recorded bat tilt, retaining signed
  axial and thickness offsets; verify any stored signed offsets agree.
- On the leading circular surface, effective collision radius R=.070 m, use
  `r=thickness/R`, axis `(h*cos(tilt),0,sin(tilt))`, normal
  `(-h*sin(tilt)*r,sqrt(1-r*r),cos(tilt)*r)`. Here h is handedness.
- Rotate axis/normal about world Z by `-h*face`, where face is the recorded sum
  of approach variation, pull bias and timing. This is a full geometric face
  yaw, **not** the live empirical .85 output-angle multiplier. The live baseline
  continues to include .85; it is not retrofitted to reproduce a physical pose.
- Infer attack angle by subtracting the live vertical contact deflection from
  recorded VLA. Reference velocity at the sweet point is `(0,v*cos(attack),
  v*sin(attack))` using recorded bat speed. Set angular velocity to
  `axis cross reference_velocity / .9 m`. Add the axial-point velocity
  `omega cross (axis*axial_offset)` at the recorded contact. The .9 m lever and
  .16 m half length follow the existing diagnostic plan defaults.
- Reuse the existing frozen finite collision parameters and the restitution
  profile at `.71 m + signed axial offset`. Caps, offsets outside the effective
  radius and points outside the profile are counted, never clamped or given
  replacement values. Keep exclusions by contact quality to expose selection.
- Incoming velocity is the pitch's live constant-acceleration plate velocity.
  Reconstruct pitch spin with the existing spin-efficiency convention and zero
  gyro sign, because the gyro sign is not recorded. This omits unobserved gyro;
  it does not assert that real pitches have none.

Lengths and thicknesses are fixed in the legacy bat-local frame. The yaw/circular
normal, attack inversion, angular velocity and omitted incoming gyro are model
assumptions. Results cannot distinguish these pose assumptions from shortcomings
of the impulse model, nor validate a production replacement on their own.

## Cases and denominators

| Case | Bat motion | Tangential response | Flight inputs |
|---|---|---|---|
| stationary_normal | zero initial velocity/rotation | mu=0, et=0 | solved velocity/spin |
| moving_normal | frozen pose proxy | mu=0, et=0 | solved velocity/spin |
| moving_finite | frozen pose proxy | existing mu=.2, et=.4 | solved velocity/spin |
| moving_normal_hla_only | frozen pose proxy | mu=0, et=0 | solved HLA; recorded EV/VLA/spin |

`stationary_normal` is a finite-mass bat initially at rest, not an infinitely
massive fixed wall. It is a motion control, not a reconstruction of the legacy
elastic-reference formula. Geometry, mass, inertia and restitution are held
constant between its input and the moving inputs.

Each case has a paired **live** baseline over exactly its accepted contacts.
Never compare unrelated case means as though their eligibility were identical.
`controlled_pairs` additionally compares stationary vs moving normal and moving
normal vs finite only on the intersection of successful responses for the same
contact. These comparisons isolate motion and tangential response respectively.

`attempted` is the number with an available adapter input. It equals the sum of
statuses; `accepted` equals both paired group `all.n` values. Non-approaching,
unsupported, energy-rejected or failed flight cases remain separate. Missing
source inputs and out-of-profile inputs are in coverage, outside attempted.
`baseline_all_contacts` preserves the full reconstructed cohort for comparison.

Group distributions include EV/VLA/signed and absolute HLA/spin moments, bins
and joint EV/VLA bins, both for all contacts and for the non-foul subset.
Geometric fouls use live flight/fence and the final-angle foul test; caught foul
fly outs are not replayed. Non-foul includes HR/automatic doubles, not predicted
hits. Fielders, later throws/pitches and strategy are not replayed, so these
numbers are neither full-game forecasts nor new hit rates.

## Spin and numerical checks

For solved outgoing velocity u (unit vector) and angular velocity omega, Magnus
uses only the transverse component. Project `omega cross u` into the live flight
integrator's up/right basis to obtain spin angle; pass its magnitude as effective
RPM. Total and gyro RPM are also reported, but gyro is not passed as extra lift.
The legacy scalar batted spin has no separate total/gyro decomposition: those
moments stay missing in baseline and HLA-only cases.

Reuse the solver's energy balance, normal/tangential restitution, impulse,
friction cone and dissipation checks. Any reported identity outside [0,1e-7]
aborts the report. Keep per-check maxima in JSON. Bounded samples (first two per
quality/case) include inputs, full solver responses and both flights; they are
illustrative rather than random samples.

## Development check: 36 games x three seeds

Six generated teams, season 2026, seeds 7/42/20261108 (108 games). This is a
small development fixture, not the user's roster or a fitted target season.

- All 8,429 live contacts reconstructed; 72 were outside the fixed restitution
  profile, leaving 8,357 adapter inputs. Sixteen marginal contacts were
  non-approaching in the moving proxy, leaving 8,341 accepted moving responses.
  Exclusions were counted, not converted into misses, fouls or zero EV.
- All ordinary total/team/HR-leader report fields exactly matched the observer-off
  reference for all seeds, with the same input hash. HLA-only accepted cohorts
  preserved EV/VLA/spin moments exactly. Paired denominators/statuses and every
  all/non-foul histogram reconciled. Controlled-pair cohorts were also checked.
- Physics identity maxima were below 2.1e-12 (largest: energy balance). No identity
  failed, and no trial flight failed in this fixture.

For moving-normal vs moving-finite, the matched baseline cohort is identical:

| Quality | Matched contacts | Live geometric fouls | Moving normal | Moving finite |
|---|---:|---:|---:|---:|
| SolidContact | 5,037 | 1 | 112 | 74 |
| WeakContact | 2,525 | 1,691 | 1,683 | 1,435 |
| MarginalContact | 779 | 760 | 769 | 767 |

Moving normal alone leaves the weak/marginal foul concentration largely intact.
Tangential response reduces weak-contact fouls in this proxy, while also changing
EV/VLA/spin and adding some solid-contact fouls. This does not justify removing
the normal-reflection contribution or adopting the finite solver live.

On the WeakContact non-foul subsets, live EV averaged 41.29 m/s; moving-normal
43.68 m/s; moving-finite 39.85 m/s. These are **different non-foul memberships**
within the same overall contact cohort, not paired EV deltas. Moving-finite's
non-foul mean VLA was -11.33 degrees and effective spin 6,412 rpm, versus live
5.45 degrees and 424 rpm. Such joint changes require further evaluation. Means
over all glancing contacts can be misleading because many outputs travel back
toward the catcher; use the non-foul distributions and signed HLA bins as well.

Validation: four adapter/physics tests, three finite-solver fixture tests, one
existing Intent-collision fixture test and thirteen example tests passed. Scoped
rustfmt and git diff --check passed. Source database SHA256 was unchanged:
`519b8ca87c7f68126cd2e3b7e599401fc7c5d09fde40140d16573856d85b3a2e`.
