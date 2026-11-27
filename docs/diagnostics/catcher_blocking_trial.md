# Catcher blocking

Catcher blocking is enabled by default in scheduled games, including
scheduled games with contact diagnostics. Set `SABERBB_CATCHER_BLOCKING=0`
for the off comparison. Setting it explicitly to `1` is equivalent to
leaving it unset. On macOS or Linux, use
`export SABERBB_CATCHER_BLOCKING=0` before running the game, or
`unset SABERBB_CATCHER_BLOCKING` to return to the default.

Blocking applies to taken pitches below the existing low wild-pitch limit. With an ordinary catcher, a near-center pitch just
below the limit is usually stopped. The chance decreases smoothly with
additional depth and lateral displacement, and increases with
`CatcherInfo.pitch_blocking`. Existing database rows and older serialized
catchers use 0.8. New catchers draw a value centered at 0.8, with a standard
deviation of 0.25 before clamping to 0 through 1.6. Fielder catching remains
a separate skill. The coefficients are provisional. A normalized location
does not identify the bounce point, so this trial uses location as a proxy.

The blocked pitch is a ball. On ball four the usual walk advances apply.
The blocked pitch is charged neither as a wild pitch nor a passed ball.
A missed block with additional advancement is a passed ball when a catcher
rated 0.8 would have at least a 50% chance of stopping the pitch. A missed
block on a harder pitch can be charged as a wild pitch. Both missed plays
use the existing loose-pitch advancement rules. The batting row stores
both the blocking result and any passed-ball charge. Runs from the passed ball onward in the same
half-inning use the existing uncertain earned-run treatment because
the log cannot replay the inning without the passed ball. Legacy rows
retain their historical scoring.
Blocking uses the game RNG after the decision to take, so game paths can
diverge even when the seed and initial DB match.

## Compare saved seasons

Use two copies of the same unplayed season DB and the same game seed.
Keep the other pitching settings identical between runs, including any
third-base risk penalty. Windows Command Prompt:

```cmd
set "SABERBB_SCHEDULED_GAME_SEED=20261127"
set "SABERBB_DB=C:\path\to\baseline.db"
set "SABERBB_CATCHER_BLOCKING=0"
cargo run --release --locked -- --process 40
python tools/diagnostics/wild_pitch_audit.py --season 2026 > catcher_blocking_baseline_2026.json

set "SABERBB_DB=C:\path\to\trial.db"
set "SABERBB_CATCHER_BLOCKING=1"
cargo run --release --locked -- --process 40
python tools/diagnostics/wild_pitch_audit.py --season 2026 > catcher_blocking_trial_2026.json
```

Compare geometric `extreme_locations`, recorded `catcher_block_attempts`
and `catcher_blocks`, advancement-based `cached_wild_pitches`, and
`passed_balls`. Use
`wild_pitch_advancement_audit.py` to inspect remaining wild pitches or
`third_base_pitch_audit.py` to compare wild-pitch runs with a runner on third.
