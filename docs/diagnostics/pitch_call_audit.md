# Pitch-call shortlist audit

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
