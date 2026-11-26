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

## Comparing upper-end pitch-location control

`SABERBB_PITCH_CONTROL_CAP` sets an optional threshold for the control value
used to sample actual pitch locations. Above that threshold, the effective
control is `cap + (control - cap) * scale`. At and below the threshold the
sampler uses the original control. `SABERBB_PITCH_CONTROL_ABOVE_CAP_SCALE`
accepts values from 0 to 1. It defaults to 0 (a hard cap). A value of 0.25
retains a quarter of the control above the threshold, preserving the ordering
of high-control pitchers. A value of 1 or an unset threshold uses full control.
The error floor and fatigue multiplier apply after converting effective control
to location error. Roster abilities, pitch calling, rotation, and the batter's
expected-pitch sample keep using the existing model.

Start from three copies of the same preseason DB. The following Windows
Command Prompt commands hold the 0–2 strategy, fatigue, location error
floor, and game seed fixed:

```cmd
set "SABERBB_SCHEDULED_GAME_SEED=20261125"
set "SABERBB_PITCH_AROUND_0_2_EDGE_SCALE=0"
set "SABERBB_PITCH_LOCATION_ERROR_FLOOR=0"
set "SABERBB_PITCH_EXECUTION_FATIGUE=1"
set "SABERBB_DB=C:\path\to\saberbb_baseline.db"
set "SABERBB_PITCH_CONTROL_CAP="
set "SABERBB_PITCH_CONTROL_ABOVE_CAP_SCALE="
cargo run --release --locked -- --process 40
python tools/diagnostics/pitch_control_audit.py --season 2026 --count-margins --require-zero-two-out > pitch_control_uncapped_2026.json

set "SABERBB_DB=C:\path\to\saberbb_cap.db"
set "SABERBB_PITCH_CONTROL_CAP=1.20"
cargo run --release --locked -- --process 40
python tools/diagnostics/pitch_control_audit.py --season 2026 --count-margins --require-zero-two-out > pitch_control_cap_120_2026.json

set "SABERBB_DB=C:\path\to\saberbb_attenuated.db"
set "SABERBB_PITCH_CONTROL_CAP=1.20"
set "SABERBB_PITCH_CONTROL_ABOVE_CAP_SCALE=0.25"
cargo run --release --locked -- --process 40
python tools/diagnostics/pitch_control_audit.py --season 2026 --count-margins --require-zero-two-out > pitch_control_attenuated_120_025_2026.json
```

For each pitcher, read `by_count_and_margin["1-2"]["Edge"]` for the
actual zone-external rate and the 2–2 transition rate. Use the same
pitcher IDs to compare three-ball entries and walks per batter faced
across trials, even if qualification changes. Compare the rates pitcher by
pitcher above the threshold to check how much variation the trial retains.
Matching seeds fix each game's initial random stream. Later plate appearances
and starter workloads can diverge when game outcomes differ.

## Inspecting saved three-ball pitch locations

Point `SABERBB_DB` at a completed simulation DB and run the audit in
Windows Command Prompt:

```cmd
set "SABERBB_DB=C:\path\to\saberbb_attenuated.db"
python tools/diagnostics/pitch_control_audit.py --season 2026 --three-ball-locations > pitch_three_ball_locations_2026.json
```

The read-only audit uses saved pitches. The `three_ball_locations` arrays in
the league and pitcher blocks group pitches by pre-pitch count, target zone,
and exact saved aim coordinates. `aim_depth` is the signed distance to the
nearest strike-zone limit along an axis in the saved coordinates. Positive
values are inside, zero is on the limit, and negative values are outside.
Standard `Wide` targets are at approximately ±2/3 on both axes, so their
depth is 1/3 regardless of which corner was selected.

Compare the number of pitches at each aim, then compare outside rates and
per-axis RMS error for the same saved aim and count. Signed mean errors
use actual location minus aim. The outside counts record swings, takes, and
missing decisions separately, along with walks for the aim group. Interpret
the rates alongside the sample size. The league groups include the pitcher
being compared.

## Inspecting how plate appearances end

Use a completed simulation DB in Windows Command Prompt:

```cmd
set "SABERBB_DB=C:\path\to\saberbb_attenuated.db"
python tools/diagnostics/pitch_control_audit.py --season 2026 --count-endings > pitch_count_endings_2026.json
```

Read `by_count_endings` in the pitcher and league blocks. Counts are measured
before the pitch. The report shows total pitches and completed plate
appearances by outcome: strikeout, fair contact, walk, or hit by pitch.
Strikeouts include separate swinging, called, and unknown-decision counts.
Fouls and runner outs that do not finish a plate appearance do not count as
endings. `ball_count_advances_without_walk` counts recorded ball increments
before ball four. Sum it across the two-ball counts to count transitions to
three balls. A count can contain multiple pitches in the same plate appearance,
so use pitches as the denominator for pitch-level rates and finished plate
appearances for the distribution of ending counts.

## Comparing 1–2 border targets

Set `SABERBB_PITCH_BORDER_1_2=1` for scheduled games to enable an additional
aim when the count is 1–2, the strategy is `PitchAround` or
`AvoidExtraBases`, and the batter passes the existing slugger check
(`slugger_option() > 0.5`). The new aims inherit the `Edge` preference and
compete with the existing calls in both shortlists. Each aim places one axis
at magnitude 0.98 and the other at the `Wide` coordinate (2/3). The existing
pitch execution model samples the actual position from that aim. The default
setting is 0.

In Windows Command Prompt, point the two trials at separate copies of the same
preseason DB with an unplayed schedule. Keep any batting-side trials fixed.

```cmd
set "SABERBB_SCHEDULED_GAME_SEED=20261126"
set "SABERBB_PITCH_AROUND_0_2_EDGE_SCALE=0"
set "SABERBB_PITCH_LOCATION_ERROR_FLOOR=0"
set "SABERBB_PITCH_EXECUTION_FATIGUE=1"
set "SABERBB_PITCH_CONTROL_CAP=1.20"
set "SABERBB_PITCH_CONTROL_ABOVE_CAP_SCALE=0.25"

set "SABERBB_DB=C:\path\to\border_off.db"
set "SABERBB_PITCH_BORDER_1_2=0"
cargo run --release --locked -- --process 40 && python tools/diagnostics/pitch_control_audit.py --season 2026 --count-endings --count-margins > pitch_border_off_2026.json

set "SABERBB_DB=C:\path\to\border_on.db"
set "SABERBB_PITCH_BORDER_1_2=1"
cargo run --release --locked -- --process 40 && python tools/diagnostics/pitch_control_audit.py --season 2026 --count-endings --count-margins > pitch_border_on_2026.json
```

The audit recognizes both orientations as `Border` from the saved aim
coordinates. In `by_count_and_margin["1-2"]`, compare outside pitches,
outcomes (including called strikeouts and swings outside the zone), advances
to 2–2, and `hard_hit_fair` per pitch. A hard-hit fair ball has a saved exit
velocity of at least 95 mph (42.4688 m/s). For the overall 1–2 effect, sum
across margins before dividing by the combined number of pitches. The
`by_count_endings` and league walk totals show how the change carries through
later counts.

## Inspecting the 2–2 transition

Point `SABERBB_DB` at the completed trial DB. In Windows Command Prompt:

```cmd
set "SABERBB_DB=C:\path\to\border_on.db"
python tools/diagnostics/pitch_control_audit.py --season 2026 --two-two-locations > pitch_two_two_distances_2026.json
```

Compare the `two_two_locations` arrays for the pitcher and league. Match on
zone and exact aim coordinates to compare RMS error and outside rate without
mixing different aims. The array also includes actual outside swings, takes,
unknown decisions, and advances to 3–2. The advance rate uses all pitches at
that aim. For the outside swing rate, divide swings by swings plus takes.
The league includes the pitcher being compared. To compare with the rest of
the league, subtract the pitcher's counts and squared-error sums first.
Each squared-error sum is the squared RMS multiplied by the pitch count.
The audit reads the saved games and does not require a replay.

Each aim row's `outside_by_distance` groups actual pitches outside the zone
into (0, 0.05], (0.05, 0.10], (0.10, 0.25], (0.25, 0.50], and above 0.50 in
normalized zone units. The distance is measured from the nearest point on the
strike zone to the actual pitch location. A pitch beyond a corner uses both
axes. Empty bands are omitted. Each band records swings, takes, unknown
decisions, swing rate among known decisions, and advances to 3–2. Sum counts
across aims before calculating an overall rate for a distance band, or match
on exact aim as well as distance band to control for the target. Subtract the
pitcher's swings and takes from the league counts for a comparison with the
other pitchers.

## Comparing irreducible pitch location error

`SABERBB_PITCH_LOCATION_IRREDUCIBLE_ERROR` adds a control-independent component
to the standard deviation of actual pitch location in scheduled games, in
the same normalized units as the existing location error. The default is 0
and the accepted range is 0 to 0.5. For a positive value, the sampler uses
`sqrt(base_error^2 + irreducible_error^2)` before multiplying by fatigue.
`base_error` includes any upper-end control attenuation and the existing
hard error floor. Combining variances preserves the ordering of distinct
base errors and requires the same number of random draws. The pitch caller,
stored control, and expected-pitch sampler keep their existing inputs.

In Windows Command Prompt, use two copies of the same preseason DB with an
unplayed schedule. Keep batting settings fixed in both runs.

```cmd
set "SABERBB_SCHEDULED_GAME_SEED=20261126"
set "SABERBB_PITCH_AROUND_0_2_EDGE_SCALE=0"
set "SABERBB_PITCH_LOCATION_ERROR_FLOOR=0"
set "SABERBB_PITCH_EXECUTION_FATIGUE=1"
set "SABERBB_PITCH_CONTROL_CAP=1.20"
set "SABERBB_PITCH_CONTROL_ABOVE_CAP_SCALE=0.25"
set "SABERBB_PITCH_BORDER_1_2=1"

set "SABERBB_DB=C:\path\to\location_error_baseline.db"
set "SABERBB_PITCH_LOCATION_IRREDUCIBLE_ERROR=0"
cargo run --release --locked -- --process 40 && python tools/diagnostics/pitch_control_audit.py --season 2026 --count-margins --two-two-locations > pitch_location_error_baseline_2026.json

set "SABERBB_DB=C:\path\to\location_error_trial.db"
set "SABERBB_PITCH_LOCATION_IRREDUCIBLE_ERROR=0.20"
cargo run --release --locked -- --process 40 && python tools/diagnostics/pitch_control_audit.py --season 2026 --count-margins --two-two-locations > pitch_location_error_trial_020_2026.json
```

Compare league walk rates and the number of qualified pitchers with zero
or one walk. Also compare pitchers who qualified in both runs to control
for changes in playing time. For the change in execution, compare RMS error,
outside rate and advances to 3–2 within the same pitcher's 2–2 aim groups.
