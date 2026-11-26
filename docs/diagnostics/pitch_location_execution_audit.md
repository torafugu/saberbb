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
