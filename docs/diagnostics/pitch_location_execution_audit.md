# Pitch location execution audit

Compare a pitcher's saved locations against all other pitchers who threw the
same pitch type. Windows Command Prompt:

```cmd
set "SABERBB_DB=C:\path\to\saberbb.db"
python tools/diagnostics/pitch_location_execution_audit.py --season 2026 --pitcher-id 423 --pitch-type Changeup > pitch_location_execution_423_2026.json
```

Each comparison row matches the exact target coordinates and the 25-pitch
appearance band. An empty peer group has zero samples and null rates. Pitch
numbers include all pitch types thrown by that pitcher in the game. A
negative mean vertical error points below the target. RMS measures the
typical size of the miss on each axis in units of the target zone's width
or height. `below_wild_limit` counts every pitch below the geometry threshold
(`y < -2.2`), regardless of whether it was scored as a wild pitch.

Add `--model` to evaluate the scheduled-game execution formula with the
current environment variables. The defaults are irreducible error 0.20,
error floor 0, no control cap, and execution fatigue off. For a fatigue-on
scenario, set `SABERBB_PITCH_EXECUTION_FATIGUE=1` before running the audit.
Set the other `SABERBB_PITCH_*` execution settings to match the season run
if they differed from the defaults. The audit records these assumptions.
Saved game rows do not record historical execution settings or historical
pitcher control. Use observed columns on their own for mixed-setting runs.

`control_components` shows the effect of capping pitcher control, applying
the floor, and combining irreducible error in quadrature. The saved
`pitch_skill_control` is shown for reference. The execution sampler uses
pitcher control and does not apply a pitch-skill control multiplier.
The model's expected counts use the probability of crossing the lower
location limit for each saved aim. They are location probabilities on
the existing schedule of pitches, without requiring a take or simulating
a new game. The no-fatigue and no-irreducible-error calculations change
one input each, holding sample sizes, targets, and workload fixed.

With fatigue enabled, the audit reconstructs pregame load from recorded
appearances and uses the count before the pitch. Missing or mismatched
appearance records leave those pitches out of the model. Check
`model.pitches` before comparing modeled and observed totals. The result
assumes the workload history was complete, ordered as scheduled, and
applicable to the pitcher at game time. This is a read-only audit.

## Compare low targets with no runner on third

Run against the saved trial DB with the same `SABERBB_DB` convention. For
example, compare player 427's four-seam fastball with the league:

```cmd
python tools/diagnostics/pitch_location_execution_audit.py --season 2026 --pitcher-id 427 --pitch-type FourSeamFastball --base-state no-third > pitch_location_427_no_third_2026.json
```

Use `by_aim_zone_and_margin` to compare `usage_rate` and
`below_wild_limit_per_1000` at both LowInside/Edge and LowOutside/Edge.
Usage divides by all pitches of the selected type and base state for the
subject or peers. Use `matched_by_aim_and_pitch_number` to compare the same
aim and 25-pitch workload band.

Change the filter to `first-or-second` for pitches with a runner on first
or second and third empty, or use `third` for third occupied. Filtered
reports include advancement-based `scored_wild_pitches`. The existing
`wild_pitches` field counts raw events, including those without additional
advancement. Lower-limit crossings are counted even on swings. Filtered
reports exclude pitches whose starting base state cannot be reconstructed
and report them in `pitches_excluded_unknown_base_state`. The default
`any` retains the unfiltered pitch population.
