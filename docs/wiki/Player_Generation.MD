# Player Generation

Player generation is implemented by `src/domain/player_factory.rs`. `src/domain/player_service.rs` loads probability data and saves generated players through `PlayerRepository`.

The CLI player-generate mode in `src/main.rs` creates a `PlayerFactory`, calls `load_player_probs()` once, then calls `generate_and_save_player()` for each requested player.

## Generation Flow

`PlayerFactory::generate_and_save_player()` repeats this process for each player:

1. Generate a `Player` with `generate_player()`.
2. Assign a team with `assign_team(&player)`.
3. Save the player through `PlayerService::save_player(team.id, &player)`.

`generate_player()` builds the player in this order:

1. Load a random localized name and generate `PlayerInfo`.
2. Choose the first `FielderType` from `player/fielder_type`.
3. Convert that fielder type into a primary `Position`.
4. Optionally add extra fielder types from `multiple_fielder_type/<FielderType>`.
5. Generate `DefenseSkills` and attach the corresponding fielding, catching, or pitching info.
6. Generate `BatterInfo` for non-pitcher primary positions; primary pitchers currently get `None`.
7. Generate `RunningSkills` for every player.
8. Return a nested `Player { info, offense_skills, defense_skills }`.

## Probability Loading

`load_player_probs()` populates these probability caches:

| Cache | Service loader | Repository source |
| --- | --- | --- |
| `player_info_probs` | `load_player_info_probs()` | `gamma_param`: `player/player_info/age` |
| `running_skill_probs` | `load_running_skill_probs()` | `normal_param`: `player/running_skills/*` |
| `batter_info_probs` | `load_batter_info_probs()` | `item_weighted`: `player/batting_side`, `player/hitter_tendency`; `normal_param`: `player/batter_info/*` |
| `field_sector_weights_map` | `load_field_sector_weights_prob()` | `item_weighted`: `hitter_tendency/<HitterTendency>` |
| `fielder_info_probs` | `load_fielder_info_probs()` | `item_weighted`: `player/fielder_type`; `normal_param`: `player/fielder_info/*` |
| `pitcher_info_probs` | `load_pitcher_info_prob()` | `item_weighted`: `pitcher_info/throw_side`, `pitcher_info/arm_slot`, `pitcher_info/pitcher_style`; `normal_param`: `player/pitcher_info/*` |
| `pitch_type_map` | `load_pitch_type_prob()` | `item_weighted`: `pitcher_style/<PitcherStyle>` |
| `pitch_skill_map` | `load_pitch_skill_probs()` | `normal_param`: `pitch_type/<PitchType>/*` |

`load_player_probs()` currently loads `player_info_probs` twice. The second load overwrites the first with the same source data.

## Random Helpers

| Helper | Behavior |
| --- | --- |
| `RandomProvider::gamma(param)` | Samples `Gamma(shape, scale) + offset`. Used for age, then rounded to `u8`. |
| `RandomProvider::normal(param)` | Samples `Normal(mean, std_dev)`, applies optional skew, then `* coefficient + offset`. |
| `RandomProvider::gen_range(0, 100)` | Generates the uniform number. Current `RealRng` implementation uses an inclusive range. |
| `choose_item_weighted(items)` | Chooses one item using item weights. Weights must have a positive total; they do not need to sum to `1.0`. |
| `choose_item_if_exists(items)` | Tests each item independently and includes it when `rng.random() < item.weight`. |

## PlayerInfo

| Field | Generation |
| --- | --- |
| `id` | `PlayerInfo::new_unsaved()` sets `id = 0`; SQLite assigns the saved id. |
| `first_name`, `last_name` | `PlayerService::load_random_name()` queries `first_names` and `last_names` for the active DB language. |
| `age` | `rng.gamma(player_info_probs.age).round() as u8`. |
| `uniform_number` | `rng.gen_range(0, 100) as u8`. Uniqueness is not currently enforced. |

## Position And Fielder Type

The first fielder type is selected from `item_weighted` rows with category `player/fielder_type`.

Sample weights from `migrations/item_weighted_sample.sql`:

| FielderType | Sample weight |
| --- | --- |
| `Outfielder` | `0.24` |
| `MiddleInfielder` | `0.12` |
| `CornerInfielder` | `0.12` |
| `Pitcher` | `0.42` |
| `Catcher` | `0.10` |

The primary position is then assigned by hard-coded weights in `PlayerFactory::assign_position()`:

| FielderType | Primary position weights |
| --- | --- |
| `Outfielder` | `RF` 0.32, `CF` 0.32, `LF` 0.32, `DH` 0.04 |
| `MiddleInfielder` | `SS` 0.48, `SB` 0.52 |
| `CornerInfielder` | `FB` 0.5, `TB` 0.4, `DH` 0.1 |
| `Pitcher` | Always `P` |
| `Catcher` | Always `C` |

Extra fielder types are loaded from `multiple_fielder_type/<first FielderType>` and tested independently with `choose_item_if_exists()`. The initially selected fielder type is always added after the optional extra fielder types.

## Defense Generation

`DefenseSkills::new(primary_position)` starts with the primary position and all optional groups set to `None`.

For each selected fielder type:

| FielderType | Generated info | Stored field |
| --- | --- | --- |
| `Outfielder` | `FielderInfo` | `defense_skills.outfielder` |
| `MiddleInfielder` | `FielderInfo` | `defense_skills.middle_infielder` |
| `CornerInfielder` | `FielderInfo` | `defense_skills.corner_infielder` |
| `Pitcher` | `PitcherInfo` with embedded pitcher `FielderInfo` | `defense_skills.pitcher` |
| `Catcher` | `CatcherInfo` wrapping `FielderInfo` | `defense_skills.catcher` |

`FielderInfo` values are generated from `normal_param` under `player/fielder_info`:

| Field | Source |
| --- | --- |
| `throw_speed` | `player/fielder_info/throw_speed` |
| `running_speed` | `player/fielder_info/running_speed` |
| `reaction` | `player/fielder_info/reaction` |
| `prep_time` | `player/fielder_info/prep_time` |

## Batter Generation

The factory generates `BatterInfo` only when the primary position is not `P`.

| Field | Generation source |
| --- | --- |
| `batting_side` | Weighted choice from `player/batting_side`. |
| `swing_speed` | `normal_param`: `player/batter_info/swing_speed`. |
| `base_launch_angle` | `normal_param`: `player/batter_info/base_launch_angle`. |
| `consistency_sigma` | `normal_param`: `player/batter_info/consistency_sigma`. |
| field-sector weights | Choose `HitterTendency` from `player/hitter_tendency`, then load weights from `hitter_tendency/<HitterTendency>`. |

Current field-sector assignment in `assign_batter_info()`:

| `FieldSector` | Assigned field |
| --- | --- |
| `Pull` | `weight_pull` |
| `Center` | `weight_center` |
| `Opposite` | `weight_opposite` |
| `FoulOpposite` | `weight_foul_pull` |
| `FoulPull` | `weight_foul_opposite` |

## Running Generation

Every generated player receives `RunningSkills`.

| Field | Source |
| --- | --- |
| `speed` | `normal_param`: `player/running_skills/running_speed` |
| `lead_distance` | `normal_param`: `player/running_skills/lead_distance` |
| `start_reaction` | `normal_param`: `player/running_skills/start_reaction` |

## Pitcher Generation

Pitcher data is generated when the selected fielder types include `FielderType::Pitcher`.

| Field | Generation source |
| --- | --- |
| `throw_side` | Weighted choice from `pitcher_info/throw_side`. |
| `arm_slot` | Weighted choice from `pitcher_info/arm_slot`. |
| `pitcher_style` | Weighted choice from `pitcher_info/pitcher_style`. |
| `height` | `normal_param`: `player/pitcher_info/height`. |
| `extension` | `normal_param`: `player/pitcher_info/extension`. |
| `velocity` | `normal_param`: `player/pitcher_info/velocity`. |
| `control` | `normal_param`: `player/pitcher_info/control`. |
| `stamina` | `normal_param`: `player/pitcher_info/stamina`. |
| `injury_proneness` | `normal_param`: `player/pitcher_info/injury_proneness`. |
| `clutch` | Current factory code uses `player/pitcher_info/injury_proneness`; `player/pitcher_info/clutch` is loaded but not used here. |
| `hpp` | `normal_param`: `player/pitcher_info/hpp`. |
| `platoon_splitting` | `normal_param`: `player/pitcher_info/platoon_splitting`. |
| `delivery_motion_time` | `normal_param`: `player/pitcher_info/delivery_motion_time`. |
| `pitch_skills` | Generated from selected pitch types for the pitcher's style. |
| `fielder_info` | The generated `FielderInfo` for `FielderType::Pitcher`. |

Pitch inventory is selected by `assign_pitch_skill()`:

1. Load pitch-type probabilities for the chosen `PitcherStyle` from `pitcher_style/<PitcherStyle>`.
2. Include each pitch type independently with `choose_item_if_exists()`.
3. For each included pitch type, generate `PitchSkill` fields from `normal_param` under `pitch_type/<PitchType>`.

## Team Assignment

`assign_team()` calls `PlayerService::next_team(player.defense_skills.position)`.

| Step | Behavior |
| --- | --- |
| Primary lookup | `next_player_dist_team(position)` returns the team with the fewest players at that primary position. |
| Fallback | If the lookup returns `AppError::NotFound`, `next_random_team()` selects a random team. |
| Other errors | Wrapped as `AppError::Internal` with context. |

## Persistence

Generated players are saved by `SqlPlayerRepository::insert_player()` in one transaction.

| Table | Saved data |
| --- | --- |
| `player_info` | Team id, first name, last name, age, uniform number. |
| `batter_info` | Present only when `offense_skills.batter` is `Some`. |
| `running_skills` | Saved for every generated player. |
| `defense_skills` | Primary position. |
| `fielder_info` | One row per generated defensive skill group. |
| `pitcher_info` | Present only when `defense_skills.pitcher` is `Some`. |
| `pitch_skill` | One row per generated pitch skill for pitchers. |
