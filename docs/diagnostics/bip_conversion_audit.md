# Ball-in-play conversion audit

`offense_season_check --bip-diagnostics` observes completed game results without
changing the simulation, sampling RNG, collecting contact states or saving data.
It separates all-contact quality from the terminal BIP population, then groups
BIP outcomes by trajectory, EV/VLA, position and fielding path/type/error.

## Definitions and limitations

- BIP denominator: Single, Double, Triple, Out, FieldersChoice, DoublePlay. A DP
  is one play. HR/K/nonterminal foul records are excluded. Automatic ground-rule
  doubles remain in the denominator and are counted separately.
- Each timing mean has its own n and null when unavailable. A record without a
  targeted runner or a positive runner time cannot establish a timing margin.
  Lead-runner targets are not mislabeled as batter targets.
- First-base batter times include the same acceleration lag as live judging.
  Second/third no-lag times describe the legacy RunningPlan selection model.
  Current planning includes acceleration, as described in
  [the clock correction](contact_direction_audit.md). These
  are not new throw-to-second/third simulations or proof of safe advancement.
- Error-free DirectFly/BounceCatch positions are reconstructed with the same
  coarse estimator as fielding, not by replaying physical ball flight. Errors
  and FinalPickup are excluded: their handling delays obscure the catch instant.
- Straight-line catch/final distance differences do not evaluate cutoff choices,
  receiving, run-to-base alternatives or counterfactual safe/out outcomes.
- Air risk-ok probes are control searches, not guaranteed successful catches.
- Examples are the first four per flag; they are bounded illustrations rather
  than a representative sample. Missing flag keys mean zero occurrences.

## Development check: 36 games per seed

The generated six-team development roster was simulated at seeds 7,42,20261108,
36 games each (108 total). All ordinary report fields were identical with and
without the observer. Every grouping partition was checked against its BIP
summary denominator. The source DB SHA256 remained:
`519b8ca87c7f68126cd2e3b7e599401fc7c5d09fde40140d16573856d85b3a2e`.

| Seed | Terminal BIP | Non-HR hits | Single after out trying second | Actual second-attempt time minus plan time |
|---|---:|---:|---:|---:|
| 7 | 1,873 | 931 | 45 | 0.700 s (65 timed attempts) |
| 42 | 1,798 | 864 | 39 | 0.700 s (52 timed attempts) |
| 20261108 | 1,891 | 909 | 55 | 0.700 s (67 timed attempts) |

Pooled: 5,562 terminal BIP and 2,704 hits. SolidContact accounts for 4,749 BIP
(85.38%). This concentration partly reflects removal of weak contacts as fouls:

| Contact quality | All recorded contacts | Terminal BIP | Nonterminal fouls | HR |
|---|---:|---:|---:|---:|
| SolidContact | 4,874 | 4,749 | 4 | 121 |
| WeakContact | 2,527 | 789 | 1,706 | 32 |
| MarginalContact | 786 | 24 | 762 | 0 |

Do not interpret the BIP SolidContact fraction alone as an unconditional contact
skill distribution. The all-contact funnel exposes the conditioning on fair
terminal outcomes. This is a development fixture, not the user's roster.

All 139 singles satisfying the second-base plan threshold also recorded the
batter out at second; none were found without a batter-out explanation. That
supports the single-credit classification, but exposes different plan/judgment
clocks: RunningPlan uses no acceleration lag, while the actual attempt uses an
extra 0.7 seconds. Aligning those clocks would affect running decisions and outs;
it is not evidence that the same plays should become doubles.

The throw timing code references `ball.final_position` instead of the available
`catch_position`. Among eligible nonerror catches with timed grounder-play
records, the mean straight-line distance difference to the recorded target was
17.32 m, 16.44 m and 15.70 m across the three seeds. Distances can be negative.
No first-base safe play with a >10 m positive gap was observed in this fixture;
this audit does not establish that correcting the origin would explain high
BABIP. A matched chosen-throw recalculation is needed before changing this code.

Only four non-HR hits across these seeds had a risk-ok air probe without a
successful direct catch. No targeted timing/ruling mismatches, unexplained
outfield singles beyond the second-base threshold, or single records with the
batter already on second/third in the recorded snapshot were observed.

## Next input

Run the same observer against the user's current roster before changing EV,
contact tolerances, fielding ability or running coefficients:

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --example offense_season_check -- --season 2026 --games 360 --bip-diagnostics > bip_audit_360.json
```

The result can distinguish hard-contact/foul selection, fielding availability,
throw/running timing and extra-base attempt outcomes. HBP calibration remains a
separate investigation.
