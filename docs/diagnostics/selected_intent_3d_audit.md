# Selected Intent forward 3-D audit

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --example offense_season_check -- --season 2026 --games 360 --selected-intent-diagnostics > selected_intent_3d_360.json
```

This mode collects the existing contact-state diagnostic pipeline for one game
at a time, observes it in memory and discards the GameResult after aggregation.
It opens SABERBB_DB read-only, never calls a result repository writer and does
not persist contact states. The ordinary game result still comes from the live
resolver; no shadow result feeds back into Intent selection or game progression.
Without this flag, the example continues to collect no contact states.

## Cohort and forward path

The source denominator is every recorded **swing attempt**, including legacy
SwungAndMiss, rather than only legacy contact/fair-ball records. Takes have no
contact state and are outside this denominator. An attempt can lack a completed
legacy batting record (for example, an early return after a baserunning out).
These joins remain unknown; no missing batting record is labeled a miss.

Reuse the current player/manager decision already captured in the record:

`selected Intent -> observed ExecutionSwingPlan -> search_contact -> finite collision -> launch metrics`

Only one selected probe is aggregated, regardless of how many alternatives were
computed by the existing pipeline. For available execution searches, check unique
selection, observed/execution decisions, game/count identity and settings. For
geometric contacts, also check selected collision identity, decision and model
version before evaluating its response. Duplicate selection or conflicting
lineage in the evaluated path aborts. Unavailable plans/searches are counted
with their original reasons; the observer never chooses another Intent.

For a selected geometric contact, reconstruct its collision input/control/response
from the actual forward search geometry, velocity and execution phase. Require
exact equality with the stored selected evaluation. Validate normal/tangential
restitution, impulse, energy and control residuals (<=1e-7); identity failures
abort, including upstream identity/control mismatch errors. Unsupported caps,
out-of-profile points, invalid spin, non-approaching collisions and other
unavailable responses have separate counts.

No legacy-offset pose adapter is used to generate a shadow collision. The
selected forward model is still a diagnostic model with its own prediction,
execution, search, finite-bat and pitch-spin assumptions. It is not measured truth
or a calibrated production replacement.

## Agreement and distributions

Each swing contributes one search status and one final collision/flight status.
`by_intent` and `by_intent_and_legacy_quality` partition the source attempts.

- `contact`: search succeeded, including a cap contact unsupported by the solver.
- `no_contact`: a completed search explicitly found no contact.
- `unresolved`, `overlap_at_start`, and `unavailable:*`: retain these separately.
  None is treated as a completed miss or a contact.
- The agreement matrix crosses legacy contact/miss/unknown with search categories.
  Binary agreement includes only known legacy labels and contact/no_contact.
  It is null if the binary denominator is zero; it is not overall Contact%.

`outcomes` includes every accepted selected 3-D flight, including those on legacy
misses and unknown legacy joins. `paired_live` and `paired_3d` compare the same
subset of accepted 3-D flights whose legacy contact launch also reconstructs.
These paired groups exclude legacy misses; do not use them to summarize all swings.
`paired_baseline_statuses` exposes why an accepted flight lacks a paired baseline.

`by_intent_and_contact_band` covers every successful geometric search, even if
collision or flight is unavailable. Bands describe side/cap region, signed axial
location and world-normal Z. Geometry summaries include release-relative contact
time offset, centerline velocity magnitude and world X/Y/Z contact position.

All/non-foul launch distributions reuse the moving-collision report's EV/VLA,
signed and absolute HLA, spin, histograms and joint EV/VLA bins. Each histogram
has its own declared denominator; non-foul subsets can differ between models.
Non-foul includes HR/automatic doubles and is not a predicted hit rate.

## Flight and resource limits

Launch EV/HLA/VLA/spin come directly from the selected finite response. Project
outgoing angular velocity into transverse and gyro components; only transverse
RPM drives Magnus in the existing flight integrator. Total/gyro remain separate.

The flight integrator starts at its existing home-plate origin and **.90 m impact
height**, not the searched world contact point. That point is separately reported.
Thus flight/foul categories remain a standardized launch comparison; this mode
is not a complete 3-D game replay. Fielding/running/alternate game events are not
simulated from shadow balls. Missing/failed flights never become zero-EV balls.

Recording invokes the existing deterministic sensor-noise generator keyed by
(game_id,count_seq). It does not consume the game's random stream. The observer
itself draws no RNG. Verify ordinary total/team/HR-leader payload parity across
seeds, and use fresh summaries rather than mixing shadow figures into saved stats.

Raw states are retained only in the current game's GameResult. Only aggregates
and the first two samples per Intent/search-category/final-status survive;
samples contain selected search/evaluation, not full per-swing plans or all
candidate probes. Samples are illustrative, not random. Output has an explicit
source version, finite-model name and search settings.

## Development validation: 12 games x three seeds

Generated six-team roster, season 2026, seeds 7/42/20261108 (36 complete games).
All total/team/HR-leader payloads exactly matched observer-off runs with the same
input hash. This fixture is not the user's roster and is not a calibration target.

There were 4,666 recorded swing attempts: 3,553 geometric contacts, 1,070 completed
no-contact searches, and 43 unavailable executions (`execution_starts_before_plan`).
No unresolved or initial-overlap searches occurred here; those categories remain
supported and are not treated as misses. Legacy joins were unique for 4,396
attempts and absent for 270, which remained unknown.

| Known binary comparison | 3-D contact | 3-D no contact |
|---|---:|---:|
| Legacy contact | 2,198 | 610 |
| Legacy miss | 1,133 | 412 |

Binary agreement is 2,610 / 4,353 (59.96%); the 270 unknown joins and 43 unavailable
searches are outside this denominator. This is agreement between two models,
not evidence that either model is accurate. In particular, evaluating only
legacy-contact records would miss 1,133 geometric contacts found on legacy misses.

Of 3,553 selected geometric contacts, 51 cap contacts and 37 out-of-profile
contacts had no supported collision response. The remaining 3,465 had accepted
flights; 699 were geometric fouls. These are standardized flight categories, not
new game results or hit probabilities. No physics-identity or flight errors
occurred. Selected input/control/response reconstruction passed throughout;
largest identity residual was 4.84e-12.

Source/status/agreement/geometry-table denominators, paired cohort equality and
all/non-foul histograms were reconciled. A debug binary run of these 36 games
took about 30 seconds with peak RSS 44,788 KiB on this worker. This measures this
fixture only; raw states were never accumulated across games or written to DB.

Validation: 18 example tests (including five new selected-path tests), plus the
existing full-GameResult recording-parity test (two seeds), passed. New tests
cover both Intent selections, ignored unselected alternatives, immutable source
records, missing joins, legacy-miss/3-D-contact disagreements, invalid-spin
coverage, duplicate/tampered selected probes and incomplete search handling.
Scoped rustfmt and git diff --check passed. Source DB SHA256 remained
`519b8ca87c7f68126cd2e3b7e599401fc7c5d09fde40140d16573856d85b3a2e`.

## Height-validity extension

The v2 report retains these source cohorts/outcomes and adds explicit ground
clearance counts, reference-height transitions, clear/invalid outcome partitions
and bounded invalid-position samples. See [ground validity and fixed-flight
controls](flight_and_ground_validity.md) for the exact .036 m radius boundaries.

## Drag-only paired trajectories (v3)

The default flight now uses published spin-dependent drag. Effective/transverse
spin still drives the unchanged lift model; total spin (including gyro) is passed
separately for drag. `--legacy-flight-drag` selects historical drag for both full
games and their observers within the synchronous diagnostic thread.

`drag_comparison` replays each accepted selected collision output under both drag
laws with identical launch metrics, spin components, stadium and standard .90 m
impact height. `attempted`, `paired`, `statuses`, both outcome groups and
`transitions` expose the common denominator and any errors. Its input cohort is
accepted flights under the selected game model, not all geometric contacts.
Full-game old/new paths can have different contact cohorts; this per-collision
comparison isolates flight within each report. These are shadow outbound results,
not game home runs. See [model scope and validation](spin_dependent_drag.md).
