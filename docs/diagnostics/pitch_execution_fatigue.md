# Pitch execution fatigue trial

Scheduled games can apply fatigue to each pitch with
`SABERBB_PITCH_EXECUTION_FATIGUE=1`. The default is 0. The trial uses the
existing pregame residual workload and the pitcher's own pitches already
thrown. The roster's stamina determines the reference count. Rotation,
substitution, pitch calling, and the probability of a hanging pitch use
their existing rules.

The trial uses the square of the ramp from a workload ratio of 0.50 to
1.00. At ratio 1.00 the actual pitch loses 1.0 m/s and its location-error
standard deviation is multiplied by 1.25. The ramp caps at ratio 1.25,
equivalent to a 2.25 m/s loss and a 1.5625 multiplier. The existing
speed-dependent spin calculation uses the resulting speed. The batter's
expected-pitch sample keeps the existing model.

## Paired scheduled-game comparison

Start with two copies of the same preseason DB with an unplayed schedule.
Enable the 0–2 PitchAround trial in both runs and hold all other settings
fixed. These commands also set the pitch location error floor to its default.
Run in Windows Command Prompt:

```cmd
set "SABERBB_SCHEDULED_GAME_SEED=20261125"
set "SABERBB_PITCH_AROUND_0_2_EDGE_SCALE=0"
set "SABERBB_PITCH_LOCATION_ERROR_FLOOR=0"
set "SABERBB_DB=C:\path\to\saberbb_baseline.db"
set "SABERBB_PITCH_EXECUTION_FATIGUE=0"
cargo run --release --locked -- --process 40
python tools/diagnostics/pitch_control_audit.py --season 2026 --require-zero-two-out > pitch_fatigue_baseline_2026.json

set "SABERBB_DB=C:\path\to\saberbb_fatigue.db"
set "SABERBB_PITCH_EXECUTION_FATIGUE=1"
cargo run --release --locked -- --process 40
python tools/diagnostics/pitch_control_audit.py --season 2026 --require-zero-two-out > pitch_fatigue_trial_2026.json
```

Forty rounds are 360 regular-season games for the default six-team schedule.
Check `league.zero_two.aimed_outside` in both JSON reports. The audit flag
fails if the saved games have no 0–2 pitch aimed outside. It tests the
behavior recorded in the DB. It cannot verify the environment of a past run.
The same counts appear in each pitcher's `zero_two` block, with separate
fields for the target and the actual pitch location.
Use the saved-season report's `by_pitch_count` to compare the same pitcher,
pitch type, and appearance pitch-number band across runs. Location error is
the pooled per-axis RMS between actual and called locations, divided by the
target zone's width or height. Compare league BB% and each pitcher's
three-ball transitions as well. Bands use pitches in the current appearance
and do not include pregame load. The band-level three-ball rate divides
transitions to three balls by pitches thrown with two balls.
