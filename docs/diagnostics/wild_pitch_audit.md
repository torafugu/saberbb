# Wild pitch audit

Run the audit against a DB with completed regular-season games. For Windows
Command Prompt, set the path to the saved season:

```cmd
set "SABERBB_DB=C:\path\to\saberbb.db"
python tools/diagnostics/wild_pitch_audit.py --season 2026 > wild_pitch_audit_2026.json
```

The script reads the saved DB without changing it. The JSON starts with
league totals, followed by pitchers ordered by wild pitch count. Find the
pitcher by name or player ID. For example, 156 innings appear as 468 outs.
`wild_pitches` in the audit counts raw play events, including empty-base
events and wild ball fours. `cache_difference` subtracts this raw count
from `cached_wild_pitches`. A version 7 cache uses the advancement rule
below, so a difference is expected when some raw events caused no
additional advancement.

Use the rates per 1,000 pitches in `by_aim_zone_and_margin`, `by_pitch_type`,
and `by_appearance_pitch_number` to account for how often the pitcher used
each target, pitch type, or workload band. The workload bands measure only
pitches thrown in the current game. The `events` list gives the original
count, target and actual coordinates, signed miss on each axis, which wild
pitch limits were crossed, and whether the pitch was also ball four.

In normalized zone coordinates, wild pitch limits are `abs(x) > 3.2`,
`y > 3.5`, and `y < -2.2`. The game checks these limits on takes and
can record a wild pitch with empty bases. An extreme pitch on which the
batter swings can contribute to `extreme_locations` without increasing
`wild_pitches`. A wild ball four is stored as `Walk` and contributes to the
audit's raw count if the catcher did not block the ball. See the
[catcher blocking trial](catcher_blocking_trial.md) for the recorded block counts.

## Runner advancement

After running a season, use the saved DB to audit whether recorded wild
pitches produced any advancement attributable to the pitch. For example:

```cmd
set "SABERBB_DB=C:\path\to\saberbb.db"
python tools/diagnostics/wild_pitch_advancement_audit.py --season 2026 --pitcher-id 423 > wild_pitch_advancement_423_2026.json
```

Omit `--pitcher-id` to audit the whole season. The script reconstructs bases
from the inning counts, batting outcomes, and available running snapshots.
It follows the current simulator's transitions, including its handling of
failed steals and tag-ups. It checks the reconstructed third-base occupancy
against runs scored on each recorded wild pitch. Unknown or inconsistent
events are reported separately. The database is read-only.

`with_wild_pitch_advancement` counts pitches on which at least one runner
advanced because of the pitch. On ball four it counts only advancement
beyond a normal walk. The version 7 pitching cache counts these
advancement events as wild pitches. `forced_by_walk_only` and
`bases_empty` are excluded from pitching totals. When all events were
classified, `scored_cache_difference` compares the version 7 cache with the
advancement count. The audit does not infer new uncaught-third-strike
plays or write to the stat cache.

Opening the pitching statistics screen with the updated build refreshes
an older cache from the saved games. To refresh it from the command line
using the same `SABERBB_DB`, run:

```cmd
cargo run --release --locked -- --rebuild-stats
```
