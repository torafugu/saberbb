# FinalPickup clock and throw-origin reconstruction

The existing FinalPickup coverage path remains unchanged. Distance/lane filters,
search/selection, RNG consumption, processing delays and game rulings are not
modified. This audit addresses the prior inability to recover the pickup position
from a completion clock that already includes handling delay.

## Command and output

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --example offense_season_check -- --season 2026 --games 360 --bip-diagnostics > final_pickup_timing_360.json
```

Default seeds are 7,42,20261108: 1080 full games. Results/contact states are not
saved. The example uses the normal published drag model. Compare source-input
hash, schedule, seeds and ordinary totals with the previous BIP report.

New output:
`reports[].bip_diagnostics.throw_origin_counterfactual.final_pickup`, source
`final_pickup_timing_v1`. Existing top-level DirectFly/BounceCatch comparison
counters remain a separate cohort.

## Recorded provenance and checks

A fresh optional `FieldingCatchDiagnostics.pickup` snapshot records the selected
fielder ID/initial position, running/preparation/catching inputs, search start and
step, pickup/search instant, pickup position, handling delay, existing handling
random draw and safety-fallback flag. It is copied from the actual selected
interception; covering pickups retain the effective handler. Capturing it draws
no new randomness. It adds no SQL columns or migrations: current SQL persistence
keeps its existing columns and does **not** retain this optional snapshot.
Fresh in-memory game runs are required; missing old snapshots remain explicitly
`missing_pickup_snapshot`, never inferred by subtracting an assumed .40 seconds.
Serialized legacy diagnostics deserialize with `pickup=None`.

The observer independently replays the current .05-second pickup search from
captured initial position and skills, through ball.total_time+10 seconds. It uses
the same existing approximate ball-state estimator, not a new physical trajectory
or a bypass of candidate filters. The search must select the recorded first
reachable instant and position. It also reconstructs ordinary .40-second handling,
moving-cushion .65-second handling, fumble probability/delay from the already-used
draw, and the existing total_time+1 safety fallback. Picked point, waiting time,
error, delay, completion clock and recorded `time_to_field` must agree within
1e-7. A unique selected-candidate identity is required. Failed/absent inputs remain
status counts and are excluded from throw comparisons.

`timing` reports source/matched counts, status partitions, by-result/path counts,
pickup/delay summaries, completion residual, model height at pickup, and the
position shift that would result from mistakenly evaluating at completion time.
Examples are bounded (first three per status/special case), not random samples.

An arithmetic match is not proof of physical feasibility. Safety fallbacks occur
after the pickup search found no reachable point, yet use the historical
completion heuristic. `completion_before_required_arrival` and
`completion_minus_required_arrival_sec` compare recorded completion with
prep+straight-line movement time to the picked point. Dedicated safety-fallback
examples expose those cases. Neither movement model nor fallback is repaired here.

## Throw sensitivity

Only validated pickup snapshots with a GrounderPlay running record proceed.
Using the unique selected candidate (rather than the legacy receiver ID stored
in `catch_fielder_id`), reconstruct the original throw time from ball.final_position
with existing direct/relay/run-to-base calculators and recorded cutoff/receiver.
The baseline must reproduce recorded defense time within 1e-7 seconds.

Then change **only** the throw origin to the recorded pre-handling pickup
position. Keep pickup completion time (including its actual handling delay),
throw target, force/tag decision, participants, runner time and calculator fixed.
Do not subtract processing delay again or locate the ball at completion time.
No alternate target, subsequent throw, runner decision or game is simulated.

`throw_origin` covers ordinary successful pickup searches.
`safety_fallback_throw_origin` separately covers reproduced safety fallbacks,
whose physical arrival may be inconsistent. Each has eligible/missing/matched/
mismatch counts, valid-target denominator, same-target safe/out flips, batter-only
flips and corrected-minus-recorded time. Targets may be lead runners; a safe/out
flip is **not** a predicted change in credited hits or full-game outcomes.
Missing/nonpositive runner times do not count as comparable rulings. Examples
are bounded to four flips and identify the target runner.

Coverage identities:

- timing.source_records = sum(timing.statuses) = sum(timing.by_result)
- timing.matched + timing_not_reproduced = timing.source_records
- timing.matched = ordinary throw eligible + fallback throw eligible + no_grounder_play_record
- For each throw cohort: matched + mismatch + missing_inputs = eligible

## Validation

Tests cover moving pickups and pre-delay position, exact completion arithmetic,
tampered search/time/delay rejection, moving-cushion handling, fumbles, old missing
snapshots, selected identity, random-draw parity and no-draw fallback. Safety
fallback examples and impossible-arrival flags are checked. Existing full-game
recording parity and repository fielding persistence tests are also run.

Development roster, twelve games at each of three seeds (36 games total): all
1457 FinalPickup records reconstruct. Eighteen are explicitly separate safety
fallbacks. All original ordinary totals/team/HR-leader payloads and BIP aggregate
payloads are unchanged; existing examples differ only by the added optional
snapshot. Existing non-pickup throw comparison counters are unchanged. The source
DB SHA256 remains
`519b8ca87c7f68126cd2e3b7e599401fc7c5d09fde40140d16573856d85b3a2e`.
These are development-roster results, not the user's 1080-game dataset. The new
real-roster report is needed to measure coverage and sensitivity there.

| Development check | Count |
|---|---:|
| Reproduced pickup/completion clocks | 1457 / 1457 |
| Ordinary-search throws with reproduced baseline | 1439 / 1439 |
| Ordinary-search comparable targets | 824 |
| Ordinary-search target safe -> out | 1 (lead runner) |
| Ordinary-search batter safe/out flips | 0 |
| Separate safety-fallback throws with reproduced baseline | 18 / 18 |
| Fallback completion before modeled required arrival | 18 |

Completion residuals are zero. Fallback arrival deficits reach 11.59 seconds in
this fixture; reproducing the heuristic therefore must not be labeled a feasible
pickup. The PR preserves that existing behavior and exposes it for separate
assessment. All 499 library tests excluding the three previously established
pitching timing/location failures, and all 26 offense-example tests pass.
