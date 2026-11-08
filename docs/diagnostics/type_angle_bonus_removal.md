# Removing direct batter-type attack-angle bonuses

The live angle calculation removes the unconditional type terms: +10° for
AggressiveFreeSwinger, +2° for ClassicAnalyst, +3° for GameManager. The batter's
own base attack angle and course-dependent dynamic angle remain. The historical
random draw is consumed in the same place; the sampled value no longer changes
the physical angle. Player/manager preferences and intent decisions remain.
The Intent execution/collision path remains diagnostic shadow data.

## Full-game comparison

A local generated six-team roster and the first 360 scheduled games in 2026
were frozen and simulated at master seeds 7, 42 and 20261108. Both versions used
the identical input hash and ordinary shared GameService loop, with no contact
logging or DB writes. There were 1,080 completed games per version, no failures
or discarded games. Different outcomes naturally change subsequent random draws.
These are development fixture results, not a replay of the user's historical
441-HR season or a prediction for player 7097.

| Metric, pooled across seeds | Before | After |
|---|---:|---:|
| Batting average | .4170 | .3422 |
| HR | 7,733 | 1,576 |
| HR / PA | 7.52% | 1.65% |
| Runs / game, both teams | 23.68 | 14.59 |
| RA9 reference | 12.08 | 7.39 |
| Strikeouts / PA | 28.43% | 29.32% |
| Non-strikeout outs, including baserunning outs | 27,930 | 29,608 |
| Singles | 28,887 | 26,820 |
| Doubles | 1,627 | 757 |
| Triples | 5 | 0 |

| Seed | Before AVG | After AVG | Before HR | After HR | Before RA9 | After RA9 |
|---|---:|---:|---:|---:|---:|---:|
| 7 | 0.4186 | 0.3418 | 2580 | 502 | 12.31 | 7.21 |
| 42 | 0.4192 | 0.3442 | 2589 | 534 | 12.15 | 7.58 |
| 20261108 | 0.4132 | 0.3405 | 2564 | 540 | 11.78 | 7.37 |

Input SHA256: `5ac27f6d526fe6cf62c36d166d62e2b3aed7293b4158e863c0c720866112c6f7`

Before engine SHA256: `dfb5fa151f7489d49a71a063b1bc2e56b1adc6f745be04350d9e5e49cda61b9b`

After engine SHA256: `88e5c861630e408aa2d8650494a826c62091387da5ac79697fdbd24c3ecc48c7`

Source DB SHA256 before and after:
`519b8ca87c7f68126cd2e3b7e599401fc7c5d09fde40140d16573856d85b3a2e`

Team sums of AB, HR, runs, runs allowed, pitching outs and non-strikeout outs
matched league totals for every seed in both versions. RA9 is runs allowed *
27 / recorded outs, not an earned-run statistic. Strikeout rate rose while the
strikeout count fell (29,247 to 27,996); plate appearances also fell (102,863 to
95,474), so the rate alone does not establish a new strikeout tendency.

## Interpretation and next investigation

The type bonus materially amplified HR frequency. Removing it improves the
league's offense balance, but calibration is not complete: high batting averages
and run production remain, and doubles/triples are scarce. Keep EV, spin,
contact tolerances and defense unchanged in this patch so the next investigation
can distinguish contact precision from fielding/running conversion.

Saved historical results are preserved. To check another roster without changing
its database:

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --example offense_season_check -- --season 2026 --games 360 > offense_season_360.json
```

The default seeds are 7,42,20261108; `--games 36` is a smaller smoke test. Input
rosters/managers are read at invocation time, and saved completion/cursor state
does not restrict the selected schedule. This is fresh full-game simulation,
not historical seed/lineup reconstruction. No results, cache or cursor are saved.
