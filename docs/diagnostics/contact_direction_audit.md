# Frozen-contact direction and throw-origin diagnostics

The original all-contact funnel showed that weak/marginal contacts are largely
removed as fouls before BIP outcome statistics. The new optional observer tests
which live HLA component drives that selection, without changing contact physics.

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --example offense_season_check -- --season 2026 --games 360 --bip-diagnostics --contact-direction-diagnostics > contact_direction_360.json
```

The database is opened read-only. No game results or contact states are saved.
Each seed gains `contact_direction_diagnostics`; `--bip-diagnostics` also adds
`bip_diagnostics.throw_origin_counterfactual`.

## Direction sensitivity

For every Solid/Weak/Marginal contact (including foul/HR), decompose the version-2
HLA into cylindrical contact-normal reflection, approach variation, pull bias and
timing. The latter three include the live 0.85 face-angle scale. Apply handedness
and wrap only after combining components, as in the live model.

Reconstruct HLA and the original flight endpoint, time, maximum height and
outbound result before accepting a contact. A mismatch aborts the report, while
missing/unsupported components have a separate count. Then omit each component
individually and replay the live flight integrator with EV/VLA/spin/stadium fixed.
The `none` case is baseline. Errors in trial flights remain separate, rather
than being treated as fair balls. Each case satisfies
`geometric_foul + non_foul + replay_errors = n`.

Foul classification follows outbound early returns and, for InField, the live
final-position angle test. An InField ball that stops in foul territory is still
a geometric foul even without a foul-boundary collision. Caught foul flies can
be outs, so geometric fouls and recorded nonterminal fouls have separate counts.
Fielding is not replayed: non-foul is not a predicted hit and includes HR and
automatic doubles. Component removal is a sensitivity probe, not a replacement
physical model. Component magnitudes are sums; divide by baseline n for means.
Examples are the first six transitions, not representative samples.

## Throw-origin sensitivity

Use unique primary fielding records with error-free DirectFly/BounceCatch
positions reconstructed by the same estimator as fielding. Exclude FinalPickup
and errors: unrecorded handling delays prevent reconstruction of catch position.
Keep the recorded target, runner time, thrower, receiver and cutoff participant.
Use the live direct/relay/run-to-base calculator and require original defense
time reproduction within 1e-7 s. Change only the origin from final ball position
to the modeled catch position. Count changes to the targeted runner's ruling;
separate batter targets from lead runners. Missing/zero runner times do not
establish a ruling. Excluded plays may matter, and a target ruling flip does not
predict a complete play, hit credit, run total or alternate target choice.

A source-record issue surfaced during validation: on non-fly plays,
`GameResult::add_player_fielding` puts the receiver ID in `catch_fielder_id`.
The diagnostic therefore uses the unique selected candidate's ID/position to
identify the true thrower, and validates the reconstructed baseline. It does not
rewrite historical records or silently treat the receiver as the thrower.

## Running clock correction

Outfield advance planning now includes the same acceleration lag used by actual
batter-runner judging: 0.7 s to second and 0.9 s to third under the current model.
The prior optimistic window could trigger an attempt that the actual clock
necessarily ruled out. A batter now holds at the preceding base in that window;
existing stretch outs are not reclassified as doubles. Existing-runner advance
plans can also change. Full games may subsequently diverge even with the same
seed, so compare fresh runs and do not combine saved pre-fix/post-fix seasons.
The older BIP audit's no-lag fields remain explicitly labeled legacy references.

## Development validation (36 games x three seeds)

Generated six-team development roster, season 2026, seeds 7/42/20261108; this is
not the user's roster. All 8,187 pre-fix contacts reconstructed HLA and flight.
All 8,429 post-fix contacts also reconstructed, with zero trial replay errors.
Normal total/team/HR-leader payloads were identical with observers disabled and
enabled for all three seeds, both before and after the running clock change.

Pre-fix frozen-contact geometric foul counts:

| Quality | Contacts | Baseline | Omit contact-normal | Omit timing |
|---|---:|---:|---:|---:|
| SolidContact | 4,874 | 4 | 0 | 0 |
| WeakContact | 2,527 | 1,706 | 0 | 1,556 |
| MarginalContact | 786 | 762 | 6 | 753 |

Mean absolute normal-reflection contribution was 10.59/56.17/129.58 degrees for
Solid/Weak/Marginal contacts, versus timing contributions of 1.43/3.85/5.90 degrees.
This isolates the normal term as the dominant source of foul selection in this
fixture. It does **not** establish that zero reflection is physically correct,
or estimate a full-season result under a replacement collision model.

Throw-origin comparison before the clock fix: 1,351 eligible records, all baseline
matched, no missing participants/mismatches. Of 752 known positive-time targeted
runner records, 33 changed Safe to Out and one changed Out to Safe. Batter targets
accounted for zero Safe-to-Out changes and one Out-to-Safe change; the 33 additional
outs therefore concerned lead runners. This subset does not support attributing
the high batter hit rate primarily to throw origin. Final pickups and errors were
excluded, and full alternate plays/target choices were not simulated.

Full-game running clock comparison (fresh runs; subsequent plays can diverge):

| Metric, pooled 108 games | Before | After |
|---|---:|---:|
| PA | 9,400 | 9,762 |
| AB | 8,378 | 8,709 |
| Singles | 2,639 | 2,707 |
| Doubles | 65 | 54 |
| Triples | 0 | 0 |
| HR | 153 | 154 |
| BA | .3410 | .3347 |
| Runs | 1,541 | 1,719 |
| RA9 reference (all runs, not ERA) | 7.2905 | 8.0480 |
| Singles with batter out trying second | 139 | 0 |

The fix removes the inconsistent attempt window rather than granting extra-base
hits. Avoiding those outs can extend innings and increase scoring; it is not an
offense-balancing change. High run scoring remains unresolved.

Checks: 23 running-resolver tests, 23 batting-resolver tests and 11 example tests
passed. Changed Rust files (apart from the single visibility-only edit in the
already-unformatted batting resolver) pass rustfmt checks; git diff --check
passes. The source database SHA256 remained
`519b8ca87c7f68126cd2e3b7e599401fc7c5d09fde40140d16573856d85b3a2e`.
