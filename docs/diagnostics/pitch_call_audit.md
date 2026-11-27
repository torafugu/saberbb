# Pitch-call shortlist audit

> Configuration update: gameplay settings now live under `[simulation]` in
> `default-config.toml` (see the [template](../../default-config.toml)).
> Historical `SABERBB_*` commands below no longer configure gameplay. Use the
> lowercase name without `SABERBB_` as the TOML key, with `true`/`false` for
> boolean switches instead of `1`/`0`. Keep `SABERBB_DB` for the database path.

Use the following command from Windows Command Prompt. Set `SABERBB_DB` to
the roster and schedule database.

```cmd
cargo run --release --locked --example offense_season_check -- --season 2026 --games 360 --seeds 20261125 --pitch-call-audit > pitch_call_audit_2026.json
```

Use the audit as a standalone diagnostic mode. It opens the database read-only
and plays the selected regular-season schedule with the saved rosters. Games
start independently from seeded lineups and exercise the same pitch-call
selector as scheduled processing. The results are fresh simulations. Saved
games lack the original shortlists, so the report cannot reconstruct historical
decisions. The audit uses no additional random draws.

- `out_evaluable` counts pitches with at least one executable `Margin::Out`
  call in the full candidate set. The shortlist counts record whether any
  such call survives on each side and in the final union. All these counts
  use pitches as the unit, regardless of how many candidates a pitch has.
- `out_selected` counts pitches whose selected call aims outside the zone.
- `out_scores_above_winner_without_shortlist` counts pitches where a full-set
  outside call scores strictly above the selected call after averaging the
  pitcher and catcher scores. This helps distinguish shortlist losses from
  score losses. Equal scores are excluded.
- Both `winner_minus_best_out…score_sums` blocks use positive values for an
  advantage to the actual winner. Divide each sum by its block's `samples`
  to get the mean difference. `margin_preference` is already included in
  `location_preferences`. The finalist block covers only outside calls that
  actually reach the final union.
- Counts are grouped by count and strategy, and by pitcher ID and count.
  For example, `C02` means zero balls and two strikes. With multiple seeds,
  the audit counts are pooled across the seeded games.

## PitchAround 0–2 Edge preference trial

Use the same DB, schedule, and seeds for both commands. The new outcome fields
are available in both arms. The only policy change is a multiplier on the
pitcher and catcher's Edge preference when the selected strategy is
`PitchAround` and the count is 0–2. Start by comparing the original strength
(scale 1) with zero to measure the full effect of removing the preference.
The trial does not force the selector to choose an outside call.

```cmd
cargo run --release --locked --example offense_season_check -- --season 2026 --games 360 --seeds 20261125 --pitch-call-audit --pitch-around-0-2-edge-scale 1 > pitch_call_edge_baseline_2026.json
cargo run --release --locked --example offense_season_check -- --season 2026 --games 360 --seeds 20261125 --pitch-call-audit --pitch-around-0-2-edge-scale 0 > pitch_call_edge_zero_2026.json
```

Compare `out_selected / pitches` in `C02` and then the outcome counts for
those pitches. `outcomes` covers all calls with a recorded batting result.
`selected_out_outcomes` restricts the same counts to calls intended to go
outside the zone. Calculate the actual outside rate from
`actual_outside / recorded_pitches`, the chase rate from
`outside_swings / actual_outside`, and the whiff rate from
`outside_whiffs / outside_swings`. A zero denominator means the rate is
unavailable. The BF, K, BB, and HR counts are grouped by the count at which
the plate appearance ended. Sum across counts before dividing for a
pitcher's overall rate.

To try this in normal scheduled games, set
`SABERBB_PITCH_AROUND_0_2_EDGE_SCALE` to a value in `[0, 1]` before starting
SaberBB. Its default is 1. The read-only checker uses the CLI option.

## Saved-game outcomes by count and aim margin

Set `SABERBB_DB` to the DB containing the completed games, then run:

```cmd
python tools/diagnostics/pitch_control_audit.py --season 2026 --count-margins > pitch_count_margin_2026.json
```

For 1–2, inspect `by_count_and_margin["1-2"]["Edge"]` under `league`
or the matching pitcher in `pitchers`. Compare `pitches` across margins
to see how often each aim was chosen. `actual_outside_rate` uses the
physical pitch location. `ball_count_advances` counts balls added to the
count, including ball four. At 1–2 this shows transitions to 2–2.
The outcome counts for inside and outside locations distinguish called
balls and strikes, swinging strikes and strikeouts, fouls, fair contact
(including home runs), and other results.

The diagnostic infers Center, Wide, Edge, and Out from the engine's saved
aim coordinates. Unrecognized targets are reported as Other. Older
strikeouts with no saved swing decision use `strikeout_unknown_swing`.
