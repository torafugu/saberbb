# Diagnostics, Replay and Holdouts

As of 2026-11-06. This page maps current entry points and interpretation; linked specifications contain experiment coefficients, columns and upgrade SQL.

## Live versus diagnostic behavior

| Path | Effect on games |
| --- | --- |
| Swing/Take, 75% timing displacement, signed HLA, 10ms airborne search | Used in live games |
| Player/Manager Intent, forecast updates, swing plan/execution, 3D contact, finite-bat response | Diagnostic records; do not feed back into live batted balls or scoring |
| Speed/aim/tilt/yaw sweeps, frozen mixtures, Statcast comparisons | Hypothesis comparisons on saved data, not automatic adoption |

Diagnostic JSON lives in `player_game_contact_state.state_json`. Older records may lack newer traces; do not reconstruct missing provenance by assumption.
No contact, out-of-scope and nonconvergent states are distinct; do not collapse them into EV=0 or ordinary misses. Inspect coverage and checks first.

## Existing databases

Apply the necessary additive scripts in `migrations/upgrades/` and generate new games. `--setup` recreates tables and is not an upgrade.

| Area | Procedures and analysis |
| --- | --- |
| Batting contact/misses | [Contact logs](../../batting_contact_diagnostics.md), [miss classification](../../contact_miss_diagnostics.md) |
| Swing/Take and two strikes | [Decision diagnostics](../../swing_decisions.md) |
| Timing/direction | [75% displacement](../../timing_position_trial.md), [signed HLA](../../contact_normal_hla.md) |
| Common 3D contact | [Contact coordinates](../../contact_state_coordinates.md) |
| Fielding | [Catch](../../fielding_catch_diagnostics.md), [candidates](../../fielding_candidate_diagnostics.md), [10ms probe](../../fielding_air_probe.md), [live search comparison](../../air_catch_search_validation.md) |
| Player/Manager | [Personnel migration](batting_strategy.md) |

## Explicit-seed generation and audit

Run from the repository root. DB-dependent Python tools use SABERBB_DB. Current Rust CLIs read the app's `database_path`; configure it to the same database. Do not use database-path CLI arguments.

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --locked --bin intent_holdout -- status --rounds 40
```

Declare the seed, exclusion boundary and output paths before generation. The example boundary 0 excludes no prior data and does not establish an independent holdout.

```sh
cargo run --release --locked --bin intent_holdout -- run --seed 2026110603 --rounds 40 --exclude-through-game-id 0 --batting-profile player-context-v1 --prepare-schedule --output-dir ../people_holdout_new
python3 tools/diagnostics/seeded_holdout_audit.py --run-dir ../people_holdout_new --output-dir ../people_holdout_new_audit
python3 tools/diagnostics/intent_selection_policy.py --input-dir ../people_holdout_new_audit --output-dir ../people_holdout_new_policy --replicates 2000 --bootstrap-seed 0
```

Run saves normal game results and advances rounds. `--prepare-schedule` also creates missing future schedules and aligns the cursor. Without it, insufficient schedules fail preflight. Forty means rounds, not games.
Database writes and the manifest are not one global transaction. Preserve failed runs; do not select only their successful games as a holdout. Use new output paths and avoid concurrent schedule/game changes.

`status: complete` means generation finished; it does not establish parity, coverage or strategic effectiveness.
The audit checks frozen cases, explicit seeds, build fingerprints, exact DB-row hashes and Rust/Python recalculation. To replay one frozen case with the same build:

```sh
cargo run --release --locked --bin intent_holdout -- replay --run-dir ../people_holdout_new --game-id 123
```

Replace 123 with a manifest game ID. Replay does not write to the database. See [seeded holdouts](../../seeded_intent_holdout.md) for failure and reproducibility details.

## Configuration sources

| Source | Purpose |
| --- | --- |
| default | Fixed default diagnostic objectives/preferences |
| mixed-id-v1 | Reproducible mixed assignments by ID, not inferred personalities |
| `--batting-config FILE` | Explicit team objectives/player preferences copied and hashed before generation (configured-v1) |
| player-context-v1 | Situational decisions from persisted Player.batting_character and Team.manager |

Do not combine batting-config with an explicit batting-profile. To generate balanced random assignments for existing IDs:

```sh
cargo run --release --locked --bin intent_holdout -- config-template --assignment-seed 2026110601 --output-file ../batting_config_new.json
```

Assignment seed is independent of simulation seed. This creates diagnostic configuration without changing personnel records. See [explicit configuration](../../explicit_batting_configuration.md), [random configuration](../../explicit_batting_configuration.md) and [personnel decisions](../../player_manager_batting.md).

## Interpretation and sharing

For personnel runs share `rust_collision_response_batters.csv`, `rust_collision_response_batter_coverage.csv`, `rust_collision_response_checks.csv` and `rust_collision_response_metadata.json`; inspect `batter_decisions.csv` for decision traces.
For policy comparisons share the four `intent_selection_policy_` summary/coverage/checks/metadata files.

Policy per_pitch rates use diagnostic pitches with both candidates present, not every pitch in the games. Fair is a direction test; the line-drive angle window is 10°≤VLA<30°, not a hit or scoring classification.
Paired whole-game bootstrap preserves candidate pairing but does not eliminate repeated-player/team dependence across games. Individual batter rates are not causal strategy effects.
Counts must match exactly; floating game sums use residuals per observation. Changing weights or boundaries after viewing results creates a new hypothesis requiring a frozen rule and unused seed/sample.

Details: [Rust collision response](../../rust_collision_response.md), [game bootstrap](../../rust_intent_game_bootstrap.md), [policy comparison](../../intent_selection_policy.md).
