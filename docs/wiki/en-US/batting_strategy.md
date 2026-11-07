# Batting Strategy, Personalities and Managers

As of 2026-11-06. Sources: [batting_personnel.rs](../../../src/domain/shared/batting_personnel.rs), [batting_strategy.rs](../../../src/domain/strategy/batting_strategy.rs), and [personnel repository](../../../src/repositories/batting_personnel_repository.rs).

## Persistent data versus decisions

| Layer | Ownership, lifetime and purpose |
| --- | --- |
| BatterCharacter | Persistent `Player.batting_character`, saved separately from skills |
| BatterTendencies | Contact priority, line-drive priority and adaptability derived from character |
| SituationalBattingPreferences | Weights and preferred Intent regenerated from pre-pitch count, outs and runners |
| Manager | Independent person with id, name and batting_policy |
| ManagerInstruction | Situational objective and reason, without a concrete aim location |
| BattingIntentDecision | Intent chosen from preferences and instruction; preserves the batter's preference |
| Planner | Generates aim and swing plan from Intent and forecast information |

Intent and situational Preferences are not fixed Player attributes. They are separate from existing BatterType/PlateApproach swing/take behavior.
Normal GameService records the new path as diagnostics; live contact, batted balls and swing/take decisions still use the existing path.

## Character and situational preferences

| JSON/CLI value | Contact priority | Line-drive priority | Adaptability |
| --- | ---: | ---: | ---: |
| contact_oriented | 0.8 | 0.2 | 0.4 |
| line_drive_oriented | 0.2 | 0.8 | 0.4 |
| adaptive | 0.5 | 0.5 | 1.0 |
| balanced | 0.5 | 0.5 | 0.5 |

Add adaptability×0.5 to contact score with two strikes, and adaptability×0.25 with a runner on third and fewer than two outs.
Add adaptability×0.25 to line-drive score with at least two balls and no strikes, or a scoring-position runner and two outs.
Normalize the scores into weights and prefer the larger one; ties select ContactFirst. These frozen v1 coefficients are diagnostic hypotheses, not calibrated empirical estimates.

## Manager and reconciliation

`manager` stores people; `team_manager` stores current assignments. Each team has at most one current manager and each manager at most one team. Unassigned people can exist, and replacement or transfer preserves person records. `Team.manager` supplies snapshots to games and frozen cases.

| Policy | Instruction |
| --- | --- |
| let_batter_decide | Use the batter's preferred Intent |
| put_ball_in_play | Select ContactFirst |
| seek_line_drive | Select LineDrive |
| situational | Follow the priority order below |

Situational priority: (1) two strikes → PutBallInPlay; (2) runner on third, fewer than two outs → PutBallInPlay; (3) the production opportunity above → SeekLineDrive; (4) otherwise → LetBatterDecide.
No manager means NoManager and batter discretion. Explicit instructions take precedence; there is no probabilistic obedience or repeated negotiation.

ContactFirst aims at the forecast center. LineDrive aims at the center below a forecast height of 0.6m, otherwise 15mm below it in world coordinates.
Actual contact, EV, spin and outcomes never select the Intent retrospectively. Policy names do not guarantee better fair-ball rates or scoring.

## Persistence and compatibility

`player_batting_character` stores character as JSON. Old JSON, unmigrated databases and newly generated players default to Balanced. Generation does not infer character from skills or consume an extra random draw; nondefault personalities require explicit assignment.
Missing managers do not fabricate an identity. Corrupt saved values or partially present manager schemas raise errors.

For an existing database, use the additive migration rather than the resetting `--setup`:

```sh
export SABERBB_DB="/path/to/saberbb.db"
cargo run --release --locked --bin batting_personnel -- migrate
cargo run --release --locked --bin batting_personnel -- set-character --player-id 2 --character adaptive
cargo run --release --locked --bin batting_personnel -- set-manager --id 1 --name 'Manager A' --policy situational --team-id 1
```

Replace IDs with existing database values. Current Rust CLIs read the app's `database_path` configuration; configure it to match SABERBB_DB. DB-dependent Python diagnostics read SABERBB_DB. Do not pass database paths as CLI arguments.
The migration preserves data and is repeatable. Omitting team-id creates/updates only the person; assigning another team transfers the same manager. There is no TUI editor yet.

Saved swing states contain `batting_selection`: character, situation, weights, manager, instruction reason and decision. Decisions run for all pitches, including takes, but this trace is recorded on saved swing states.
See [the detailed personnel specification](../../player_manager_batting.md) and [generation/audit workflow](diagnostics.md).
