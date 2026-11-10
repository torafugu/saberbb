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
  and legacy FinalPickup records are excluded from that reconstruction. Fresh
  FinalPickup snapshots now have a separate [pickup timing audit](final_pickup_timing.md).
- Straight-line catch/final distance differences do not evaluate cutoff choices,
  receiving, run-to-base alternatives or counterfactual safe/out outcomes.
- Air risk-ok probes are control searches, not guaranteed successful catches.
- Examples are the first four per flag; they are bounded illustrations rather
  than a representative sample. Missing flag keys mean zero occurrences.

## Single and out rates for similar batted balls

Use the same `SABERBB_DB` as in the full-game geometry sweep.

```sh
cargo run --locked --release --example offense_season_check -- --season 2026 --games 360 --timing-geometry-sweep --bip-diagnostics > bip_conversion_sweep_360.json
```

Read `timing_geometry_trials[].reports[].bip_diagnostics.tables.by_trajectory_ev_vla_hla`.
Without the geometry sweep, the same table is under `reports[]`. Each BIP
belongs to one cell. A cell combines the live spin-adjusted trajectory
(Grounder, Liner, Fly, PopUp), exit speed, vertical launch angle and
horizontal launch angle. Speed edges in m/s are 20, 30, 35, 40, 42.4688
(95 mph), 45, 50 and 55. Vertical angle edges in degrees are −10, 0, 10,
25 and 50. Horizontal angle edges in degrees are −45, −30, −15, −5, 5, 15,
30 and 45. Lower edges are inclusive and upper edges exclusive. Direction
is the initial stadium-relative angle, negative toward left field and
positive toward right field, with zero toward center. Records without a
known stadium-relative angle go into `HLA:unknown`.

The cell's `single_rate` divides singles by `n`. The `out_on_play_rate`
uses Out, FieldersChoice and DoublePlay in the numerator. It counts batting
result codes once per BIP. A play scored a single can still record a
baserunning out. The cell also contains the existing fielding and running
timing summaries, with separate sample counts for each timing.

Compare the cell frequencies (`n` divided by all BIP) for changes in
batted-ball mix. Compare the single and out rates for the same cells for
changes in conversion. Pool seeds by summing counts before dividing.
For comparisons restricted to shared cells, report the retained BIP counts
and weight both conditions with the same cell mix when comparing conversion.
Sparse cells and unknown directions warrant separate reporting. A difference
within a cell can still reflect spin, position within the bin, base state or
roster matchups. The grouped rates alone cannot attribute it to a fielding
or running rule.

## First-base timing on hard, low-angle grounders

Set `SABERBB_DB` to the same input database as the conversion sweep.

```sh
cargo run --locked --release --example offense_season_check -- --season 2026 --games 360 --bip-diagnostics > right_field_first_cover_360.json
```

Read `reports[].bip_diagnostics.hard_grounder_first_base`. It includes
terminal BIP classified as Grounder with EV in [45, 50) m/s and VLA in
[0, 10) degrees. Its `n` is the sum of those cells across all HLA bands.
Group keys contain target category, catch type, batting result and phase,
separated by vertical bars.

- `batter_first_observed` uses recorded runner and defense times when the
  primary GrounderPlay record targets the batter at first. A tie is safe.
- `other_target`, `target_unrecorded` and `no_primary_grounder_record`
  compare fielding clocks with the roster-derived, acceleration-aware batter
  time to first. The actual delivery to first is unobserved in these groups.
  Later double-play legs are outside the primary comparison.

A pickup snapshot is usable when its nonnegative pickup and handling times
sum to the recorded field completion time within 1e-7 s. With usable clocks,
the phase is the first checkpoint at or after the runner reaches first.

| Phase | Recorded evidence |
|---|---|
| `pickup_at_or_after_runner` | The runner already arrived at pickup. |
| `handling_crossed_runner` | Pickup preceded the runner and field completion did not. Handling includes any recorded fumble delay. |
| `field_complete_late_pickup_unknown` | Field completion was late and the pickup/handling split is unavailable. |
| `delivery_crossed_runner` | Field completion preceded the runner and defense completion did not. |
| `defense_completed_before_runner` | Observed defense completed before the runner. |
| `field_complete_before_runner_delivery_unobserved` | Field completion preceded the modeled runner, with no observed primary delivery to first. |

Missing clocks and inconsistent observed defense clocks have separate
categories. Delivery time is defense completion minus field completion.
It can include transfer, throw, relay, receipt or run-to-base time.
It cannot separate those components or determine whether throwing to first
would have worked when the actual play targeted another base. Every timing
summary has its own `n`.

### Modeled one-base play for unobserved singles

The same command also fills `hard_grounder_first_base.unobserved_first_trial`.
It covers singles in `field_complete_before_runner_delivery_unobserved`.
Group keys contain the original target category, infield/outfield role,
fielder position and catch type. Eligible plays partition into
`modeled_out`, `modeled_safe` and the reason counts in `missing`.

The original field completion time and acceleration-aware batter arrival
time stay fixed. FinalPickup uses the recorded pickup position and checks
the pickup/handling clock and selected fielder ID. For an error-free
BounceCatch, the existing fielding estimator supplies the origin at the
recorded catch time. The report counts these origin sources separately.
The selected fielder is identified by candidate diagnostics and gets
throwing and running skills from the source roster.

The trial assumes first base is covered. It uses the existing direct
throw timing without a tag penalty. For infielders it also evaluates
carrying the ball to first and chooses the faster option. Outfielders
use a direct throw. Completion minus batter arrival is negative for an
out and zero or positive for safe. Margin bands show how close those
modeled results are.

The trial does not account for a different cover assignment, relay choice,
throw errors or the value of retiring a lead runner. It does not modify
the original play or estimate changes in batting statistics.

### Right-field bounce catches and first-base cover

In the `target_unrecorded|outfield|RF|BounceCatch` group, `right_field_cover`
compares four scenarios on the same eligible singles. These are hard
grounders whose fielding completed before the modeled batter arrival.
The primary running record contains no target runner ID.

1. Previous estimated catch position, with first base already covered.
2. The actual selected catch position, with first base already covered.
3. The actual position, with the first baseman reacting at contact.
4. The actual position, with the first baseman reacting at the catch.

Fresh games capture the selected catch position and active first baseman's
initial position, prep time and running speed in memory. Each cover scenario
uses prep time plus straight-line travel time to first. The modeled
completion is the later of direct ball arrival and cover arrival.
All scenarios use the existing throw calculator and keep the same runner
and recorded field completion times. Ties are safe. Counts include outs
with at least 0.25 seconds of margin and cases where cover arrives after the
ball. The position difference compares the old estimated origin with the
position returned by the actual fielding calculation.

Missing or invalid snapshots are counted separately. The cover clocks are
scenarios, since the simulator does not track the first baseman's actual
cover run on these plays. The trial assumes a clean throw and reception,
with no relay or alternative cover fielder. It does not change gameplay.

### Experimental right-field force play in full games

Set `SABERBB_DB` to the same input database used for the first-base cover audit.

```sh
cargo run --locked --release --example offense_season_check -- --season 2026 --games 360 --right-field-first-comparison > right_field_first_rule_360.json
```

The comparison runs both policies on the same schedule and game-start seeds.
`right_field_first_trials` contains the baseline and experimental reports.
The enabled reports include `right_field_first_outs` per seed. The new rule
only replaces a conventional Single with no target runner, after a clean
right-field BounceCatch on a Grounder with EV in [45, 50) m/s and VLA in
[0, 10) degrees. The original rule is evaluated first.

The existing direct throw timing and captured first-base cover inputs are
used. The cover starts at the catch. Both ball and receiver must arrive
at least 0.25 seconds before the batter. The rule uses the existing force
play at first to advance any existing runners and cancels scoring if the
batter is the third out. The model assumes clean delivery and reception.
Games may diverge after the first changed play, so the number of trial
outs and the net difference in singles are separate measurements.
Ordinary simulation uses the baseline policy.

## Contact offset and exit speed

Read `reports[].bip_diagnostics.contact_offset_ev` for the two-dimensional
grid of absolute length and thickness offsets. It covers the recorded adjusted
SolidContact, WeakContact and MarginalContact population. Its length boundaries
are 2, 5, 10 and 16 cm. Thickness boundaries are 0.5, 1, 2, 2.5, 4, 5.5 and
7 cm. Keys use meters with inclusive lower and exclusive upper bounds.
Missing or nonfinite offsets have an `unknown` bin.

Each populated cell counts contact quality, recorded fouls, terminal BIP and
home runs. `all_contact_ev` includes recorded fouls. `bip_plus_hr_ev` uses
terminal BIP and home runs. Each speed group reports EV moments, the count at
95 mph (42.4688 m/s) or above and its share of valid speed samples. Use
`ev_ms.n` as the denominator when pooling reports across seeds. Missing or
invalid speeds are excluded from EV rates. Empty denominators are null.

Run on the same input DB and seeds to locate where high-speed balls enter the
distribution. The grid groups existing outputs by offset and does not itself
estimate a causal effect of changing the contact model.

```sh
cargo run --locked --release --example offense_season_check -- --season 2026 --games 360 --bip-diagnostics > contact_offset_ev_360.json
```

## Paired timing-to-position scales

Set `SABERBB_DB` to the input database and run:

```sh
cargo run --locked --release --example offense_season_check -- --season 2026 --games 360 --timing-scale-diagnostics > timing_scale_components_360.json
```

Read `reports[].timing_scale_diagnostics`. All four conditions use the same
realized swings.

| JSON field | Contact geometry | Exit-speed depth correction |
|---|---:|---:|
| `baseline` | 0.75 | 0.75 |
| `geometry_only` | 1.00 | 0.75 |
| `depth_only` | 0.75 | 1.00 |
| `comparison` | 1.00 | 1.00 |

Geometry changes the contact decision, the offset-based speed losses, the
contact-normal angle and the geometry used for spin. The depth scale changes
only the timing displacement passed to the exit-speed power correction.
Timing error in seconds and its face-rotation contribution stay fixed. When a
baseline miss becomes contact, both conditions that change geometry share the
same independent seeded direction draw. The report counts these swings once in
`new_contacts_with_sampled_direction`.

All rates use the swing count. Near-center contact means absolute length
offset below 5 cm and absolute thickness offset below 2.5 cm. Hard hits are
at least 95 mph. Transition counts distinguish misses, geometric fouls, fair
flights and flight errors.

Each variant has a `fair_exit_speed_pairs` section against the live baseline.
`both_fair` counts swings fair in both conditions. Pool the mean exit-speed
change by dividing the sum of `delta_sum_ms` across seeds by the sum of
`both_fair`. The count is zero and the mean is null for an empty cohort.
The difference in hard fair counts is the sum of two changes:

- Among balls fair in both conditions, `up_across_95_mph` minus
  `down_across_95_mph`.
- From balls entering or leaving the observed fair cohort,
  `nonfair_to_hard_fair` minus `hard_fair_to_nonfair`. Here nonfair includes
  flight errors, which are reported separately.

The two single-factor differences may interact. The combined change minus
the sum of the two single-factor changes gives the interaction for a count
or a rate with the same swing denominator. The audit ends before fielding
and the example performs no database writes.

## Full-game timing geometry sweep

Set `SABERBB_DB` to the same input database used for the paired audit.

```sh
cargo run --locked --release --example offense_season_check -- --season 2026 --games 360 --timing-geometry-sweep > timing_geometry_sweep_360.json
```

Each geometry scale (0.75, 0.875 and 1.00) runs the same schedule and starts
each game from the same derived seed. The exit-speed depth correction uses
0.75 for all three trials. Once a play changes, subsequent game state and RNG
consumption may diverge. This measures full-game effects. The frozen-swing
audit is a separate mode.

Read `timing_geometry_trials[].reports[].total` for batting average,
`strikeouts_per_pa`, `babip`, `singles_per_pa`, home runs and runs. The
per-seed totals retain the counts needed for weighted pooling. BABIP uses
`(1B + 2B + 3B) / (AB - K - HR)` because the engine has no separate
sacrifice-fly result. `babip_balls_in_play` is its denominator. Empty rate
denominators are reported as null. The example reads the database without
saving simulated results.

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
