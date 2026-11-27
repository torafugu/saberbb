# Catcher blocking trial

Set `SABERBB_CATCHER_BLOCKING=1` to enable blocking in scheduled games.
The default is 0. The trial applies to taken pitches below the existing
low wild-pitch limit. With an ordinary catcher, a near-center pitch just
below the limit is usually stopped. The chance decreases smoothly with
additional depth and lateral displacement, and increases with the existing
catching skill. The coefficients are provisional. A normalized location
does not identify the bounce point, so this trial uses location as a proxy.

The blocked pitch is a ball. On ball four the usual walk advances apply.
An unblocked pitch follows the existing wild-pitch advancement rules.
The saved batting row stores the blocking result for each attempt.
Legacy rows have no blocking result and retain their historical scoring.
The trial uses the game RNG after the decision to take, so game paths can
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
and `catcher_blocks`, and advancement-based `cached_wild_pitches`. Use
`wild_pitch_advancement_audit.py` to inspect remaining wild pitches or
`third_base_pitch_audit.py` to compare wild-pitch runs with a runner on third.
