# Pitcher/Catcher Pitch-call Reconciliation

Sources: [pitching_strategy.rs](../../../src/domain/strategy/pitching_strategy.rs) and [GameState::process_count](../../../src/domain/shared/game_state.rs), as of 2026-11-06.

Pitcher and catcher generate separate shortlists from their `PitchingPreferences`. `reconcile_pitch_call_proposals` reevaluates the union from both perspectives; absence from a shortlist does not mean inability or refusal.

## Shared context and scoring

`PitchEvaluationContext::new(pitcher, batter, strategy, previous_call)` freezes the actors, common Strategy and previous call, caching pitch estimates, speed and usage weights. Use a new context when these inputs change.

```rust
let context = PitchEvaluationContext::new(&pitcher, &batter, strategy, previous_call);
let pitcher_proposals = context.pitcher_proposals(&pitcher_preferences, 3, 2);
let catcher_proposals = context.catcher_proposals(&catcher_preferences, 4);
let decision = reconcile_pitch_call_proposals(
    &pitcher_proposals, &catcher_proposals,
    &pitcher_preferences, &catcher_preferences, &context, 0.5,
);
```

`evaluate_pitch_call(call, preferences, context)` returns `Option<PitchCallProposal>`. Generation and reconciliation use the same evaluator. Shared terms cover usage, strategy, batter matchup and walk/hard-contact/execution risks. Individual terms cover location, margin, pitch mix, sequence and risk aversion.
Missing inventory pitches and nonfinite scores/risks return None. Merely unwanted calls remain eligible with lower scores. Pitch-type shortlist estimates are not added again to final scores.

## Reconciliation

1. Deduplicate by pitch type, target zone and margin, preserving pitcher order followed by catcher-only candidates.
2. Reevaluate each union call using both Preferences. Exclude calls without valid evaluations on both sides.
3. Maximize `(1-catcher_weight)*pitcher_score + catcher_weight*catcher_score`.
4. Break ties by pitcher score, then union input order.
5. Return None when no valid call remains.

An empty shortlist on one side can still use the other's candidates. Stored proposal scores are not trusted; at most 2N evaluations are needed for N union calls, without reenumerating every pitch/location.
Clamp catcher_weight to 0–1; nonfinite values use 0.5. Endpoints select using one side's score over the union. Averaging avoids doubling shared score terms.

`select_pitch_call` is the weight-0.5 convenience API returning `Option<PitchCall>`. Existing pitcher/catcher proposal wrappers remain available; context methods reuse estimates.

## Decision and live execution

`PitchCallDecision` records the call, combined score, both scores and reason:

- Agreement: initially present on both sides.
- ReevaluatedAgreement: initially proposed by one side and selected after reevaluation.
- PitcherChoice / CatcherChoice: selected at weight 0 / 1.

Successful decisions contain both scores. Preferences are not rewritten and proposals are not repeatedly negotiated.
Live `process_count` executes the selected call through `create_pitch_for_call`. The previous call persists within the plate appearance and resets for the next batter. No valid decision raises NoPitchCall.

## Simulation test

Use a dedicated initialized test database. Set SABERBB_DB and configure the current Rust app's database_path to the same DB. The test recreates its diagnostic table.

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo test --test simulate_pitching test_select_pitching_strategy -- --nocapture
```

The test exercises 1,000 cases from Strategy through Preferences, proposals, reevaluation and final selection. Its first half covers all 24 runner/out states and 12 counts. The second half preserves the AvoidExtraBases regression conditions while using actual pitch inventory for call generation. Noninitial counts use a previous call; this is a situational aggregate test, not a full advancing plate appearance.

`test_select_pitching_strategy` stores situation, selected call, aim, both scores, reason, proposal counts and previous call. Analyze it with [strategy percentages](../../../tests/sql/pitching_strategy_percentage.sql) and [pitch-call percentages](../../../tests/sql/pitch_call_percentage.sql).
Player generation makes frequencies variable. Assertions check feasibility, maximum combined score, both evaluations, count coverage and strategy regressions rather than fixed selection percentages.
