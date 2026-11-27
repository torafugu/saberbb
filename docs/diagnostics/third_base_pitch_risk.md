# Third-base pitch risk trial

> Configuration update: gameplay settings now live under `[simulation]` in
> `default-config.toml` (see the [template](../../default-config.toml)).
> Historical `SABERBB_*` commands below no longer configure gameplay. Use the
> lowercase name without `SABERBB_` as the TOML key, with `true`/`false` for
> boolean switches instead of `1`/`0`. Keep `SABERBB_DB` for the database path.

Scheduled games can penalize low targets when a runner is on third base.
Set `SABERBB_PITCH_THIRD_BASE_RISK_WEIGHT` to a number from 0 through 10.
The default is 0. The score penalty is the weight multiplied by
`exp(-0.5 * z * z)`, where `z` is the vertical distance from the aim to
the lower wild-pitch boundary in units of the pitch's location error.
The error includes control, configured location-error terms, and any
execution fatigue. This score term is a relative risk measure.

The score enters both the pitcher and catcher evaluations, from shortlisting
through reconciliation. Lower targets receive larger penalties at the same
execution error. All calls remain eligible, including low pitches and changeups.
The trial changes pitch selection. Pitch execution and wild-pitch scoring use
their existing rules.

Use two copies of a preseason DB with an unplayed schedule. Set the same
seed and execution settings before both runs. These Windows Command Prompt
commands use the settings from the comparison:

```cmd
set "SABERBB_SCHEDULED_GAME_SEED=20261127"
set "SABERBB_PITCH_LOCATION_ERROR_FLOOR=0"
set "SABERBB_PITCH_LOCATION_IRREDUCIBLE_ERROR=0.20"
set "SABERBB_PITCH_CONTROL_CAP=1.20"
set "SABERBB_PITCH_CONTROL_ABOVE_CAP_SCALE=0.25"
set "SABERBB_PITCH_EXECUTION_FATIGUE=1"
set "SABERBB_PITCH_BORDER_1_2=1"
set "SABERBB_DB=C:\path\to\baseline.db"
set "SABERBB_PITCH_THIRD_BASE_RISK_WEIGHT=0"
cargo run --release --locked -- --process 40
python tools/diagnostics/third_base_pitch_audit.py --season 2026 > third_base_baseline.json

set "SABERBB_DB=C:\path\to\trial.db"
set "SABERBB_PITCH_THIRD_BASE_RISK_WEIGHT=3"
cargo run --release --locked -- --process 40
python tools/diagnostics/third_base_pitch_audit.py --season 2026 > third_base_trial.json
```

The read-only audit reconstructs the base state before pitch selection.
Compare low target rates and wild-pitch runs in `runner_on_third`, along
with walks, strikeouts, hits, and home runs per completed plate appearance.
The base state used for a plate appearance is the one before its final
pitch. The audit excludes runs forced in by an ordinary walk from wild-pitch
runs. Use `--pitcher-id 423` on both runs to inspect a particular pitcher.
Differences in game outcomes can change subsequent workload and base states
even when the initial seeds match. Check the pitch counts in each group.
