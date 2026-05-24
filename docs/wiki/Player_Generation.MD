# Player Generation

Player generation is implemented in `src/domain/player_service.rs`.

## Generation Flow

`PlayerService::generate_players(num_of_players)` repeats this process for each player:

1. Load player generation probabilities from `item_prob`.
2. Pick a random first name and last name for the active language.
3. Generate age, throwing hand, batting hand, and skill modifiers.
4. Assign one defensive skill by weighted position probability.
5. Assign the player to the team with the fewest players at that position.
6. If no team has that position yet, assign a random team.
7. Save the player and defensive skill in one transaction.

## Probability Sources

Generation probabilities are stored in the `item_prob` table.

| Category | Name | Struct field | Default sample value | Meaning |
| --- | --- | --- | --- | --- |
| `player_attribute` | `age_shape` | `PlayerAttributeProb.age_shape` | `2.5` | Gamma distribution shape for age generation. |
| `player_attribute` | `age_scale` | `PlayerAttributeProb.age_scale` | `2.5` | Gamma distribution scale for age generation. |
| `player_attribute` | `age_offset` | `PlayerAttributeProb.age_offset` | `18.0` | Added to generated age before rounding. |
| `player_attribute` | `throw_lefty` | `PlayerAttributeProb.throw_lefty` | `0.2` | Chance that `throw` is `Left`. |
| `player_attribute` | `bat_lefty` | `PlayerAttributeProb.bat_lefty` | `0.4` | Chance that `bat` is `Left`. |
| `batter_skill` | `ba_skew` | `BatterSkillProb.ba_skew` | `0.2` | Skew used when generating `mod_ba`. |
| `batter_skill` | `slg_skew` | `BatterSkillProb.slg_skew` | `0.2` | Skew used when generating `mod_slg`. |
| `defensive_skill` | `uzr_skew` | `DefensiveSkillProb.uzr_skew` | `0.2` | Skew used when generating `mod_uzr`. |

`mod_speed` and `mod_control` currently use hard-coded skew constants in `player_service.rs`.

| Parameter | Source constant | Value |
| --- | --- | --- |
| `mod_speed` | `SPEED_SKEW` | `0.2` |
| `mod_control` | `CONTROL_SKEW` | `0.2` |

## Position Probabilities

Sample position weights are defined in `migrations/item_prob_sample.sql`.

| Position | Sample weight |
| --- | --- |
| `P` | `0.42` |
| `C` | `0.10` |
| `FB` | `0.06` |
| `SB` | `0.06` |
| `TB` | `0.06` |
| `SS` | `0.06` |
| `LF` | `0.07` |
| `CF` | `0.07` |
| `RF` | `0.07` |

The values are used as weights by `WeightedIndex`; they do not need to add up to exactly `1.0`.

## Random Functions

| Function | Used for | Behavior |
| --- | --- | --- |
| `age_random(shape, scale, offset)` | `age` | Samples `Gamma(shape, scale) + offset`, then rounds to `u8`. |
| `rl_random(lefty_rate)` | `throw`, `bat` | Returns `Left` when a random value is below `lefty_rate`; otherwise `Right`. |
| `skewed_normal_random(skew_level)` | Skill modifiers | Samples `normal(0, 1) + skew_level * abs(normal(0, 1))`. |
| `choose_item_weighted(items)` | Defensive position | Chooses one item based on its weight. |

## Database Tables

Generated player data is saved across two tables.

| Table | Purpose |
| --- | --- |
| `player` | Core player identity, handedness, and batting/speed/control modifiers. |
| `defensive_skill` | Player position and `mod_uzr` values. |

