# FinalPickup clock and throw-origin reconstruction

The existing FinalPickup coverage path is retained. Distance/lane filters,
ordinary pickup search, RNG consumption and handling delays are unchanged.
The follow-up reachable-fallback correction changes exhausted-search clocks and
can therefore change handler selection and game outcomes. This audit addresses the prior inability to recover the pickup position
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
`final_pickup_timing_v2`. Existing top-level DirectFly/BounceCatch comparison
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
draw, and safety fallback according to its recorded policy. Picked point, waiting time,
error, delay, completion clock and recorded `time_to_field` must agree within
1e-7. A unique selected-candidate identity is required. Failed/absent inputs remain
status counts and are excluded from throw comparisons.

`timing` reports source/matched counts, status partitions, by-result/path counts,
pickup/delay summaries, completion residual, model height at pickup, and the
position shift that would result from mistakenly evaluating at completion time.
Examples are bounded (first three per status/special case), not random samples.

An arithmetic match alone is not proof of physical feasibility. Historical safety fallbacks occur
after the pickup search found no reachable point, yet use the historical
completion heuristic. `completion_before_required_arrival` and
`completion_minus_required_arrival_sec` compare recorded completion with
prep+straight-line movement time to the picked point. Dedicated safety-fallback
examples expose those cases. The follow-up correction below repairs the fallback arrival bound; the movement
and approximate ball-state models remain unchanged.

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
whose historical physical arrival may be inconsistent. Each has eligible/missing/matched/
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

## Original diagnostic validation (before fallback correction)

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

## Reachable fallback correction

After the bounded search is exhausted, compute the same preparation plus
straight-line movement time used by the ordinary pickup search, from the
fielder's actual initial position to the stopped ball. The new pickup instant is
`max(ball.total_time, required_arrival)`, and completion remains pickup+1 second.
Keep the existing one-second fallback handling delay, no-draw behavior, fallback
classification and final-position pickup. This prevents an unreachable fielder
from beating a reachable covering fielder using an artificially short clock.
Ordinary search successes, filters, bounce/flight models and throw origins do not
change. Subsequent game paths/RNG consumption may diverge after handler/ruling
changes; the fallback itself consumes no added random values.

`PickupTimingDiagnostics.fallback_policy` is `reachable_arrival` on fresh records.
A missing policy on a historical serialized snapshot defaults to `legacy_ball_end`.
The diagnostic reproduces the recorded policy and reports
`timing.by_fallback_policy`; their counts sum to timing.matched. Changing the
policy tag alone without updating its pickup/completion times is rejected.
Missing entire snapshots remain missing. The new source label is
`final_pickup_timing_v2`. No DB columns/migrations or historical data rewrites.

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --example offense_season_check -- --season 2026 --games 360 --bip-diagnostics > fallback_fixed_360.json
```

Compare against the previous `final_pickup_timing_360.json` for matching input
hash, schedule and seeds. Confirm `completion_before_required_arrival=0` and
inspect handler-position, hit/out, run and fallback counts. Correcting an
artificially early defense does not guarantee fewer hits or runs. This change is
not a calibration of offensive balance.

Regression tests reproduce an exhausted catcher search, require arrival before
one-second handling, verify no RNG draw, and confirm a reachable covering
outfielder beats the old too-fast catcher. Ordinary pickup, fumble and wall-delay
checks remain. A compatibility test removes the policy field from a historical
snapshot and reproduces the legacy clock while flagging its arrival deficit.

### Paired development run after correction

Twelve scheduled games per seed, seeds 7/42/20261108 (36 games per model),
same input hash `ad53dfab09a5f4b8e47dd9b112751650bcdc6bd4d3676be41bd3c092d43127c0`:

| Metric | Before | Reachable fallback |
|---|---:|---:|
| Selected pickup snapshots | 1457 | 1472 |
| Reproduced pickup clocks | 1457 | 1472 |
| Selected safety fallbacks | 18 | 0 |
| Completion before required arrival | 18 | 0 |
| BIP handled by catcher | 16 | 5 |
| Recorded outs | 1929 | 1932 |
| Singles | 905 | 915 |
| Doubles | 45 | 58 |
| Home runs | 4 | 5 |
| Runs | 464 | 488 |
| Batting average | .3284 | .3339 |
| RA9 reference | 6.4946 | 6.8199 |

BIP-handler counts and full-game metrics are seeded outcome comparisons, not
identical-play cohorts: changed selections/rulings alter subsequent game paths.
The fixed regression separately proves the handler-selection correction for an
identical ball/roster. A zero count of selected fallbacks does not mean none were
evaluated: unreachable candidates now lose to reachable covering fielders.
The source development DB SHA256 is unchanged. These numbers are not the user's
1080-game dataset, and this correction does not solve the remaining high offense.
