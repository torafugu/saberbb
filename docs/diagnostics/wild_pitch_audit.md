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
Check `cached_wild_pitches` against `wild_pitches` and use `cache_difference`
to identify any disagreement between the pitching stat cache and the saved
pitch results.

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
`wild_pitches`. A wild ball four is stored as `Walk` and contributes to
both the walk and wild pitch counters.
