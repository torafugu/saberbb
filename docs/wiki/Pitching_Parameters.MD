# Pitching Parameters

Pitching-related structs are defined in `src/domain/shared/player.rs`. They are not currently generated or persisted by `PlayerService` and `SqlPlayerRepository`.

## Pitch Types

| Value | Meaning |
| --- | --- |
| `FourSeamFastball` | Four-seam fastball |
| `TwoSeamFastball` | Two-seam fastball |
| `Cutter` | Cutter |
| `Curveball` | Curveball |
| `Slider` | Slider |
| `Sweeper` | Sweeper |
| `Changeup` | Changeup |
| `Forkball` | Forkball |
| `SplitFingerFastball` | Split-finger fastball |
| `Knuckleball` | Knuckleball |

## Pitcher Base Skill

| Parameter | Type | Description |
| --- | --- | --- |
| `mod_velocity` | `f64` | Overall velocity modifier. |
| `mod_control` | `f64` | Overall pitching control modifier. |
| `mod_stamina` | `f64` | Stamina modifier. |
| `mod_injury_proneness` | `f64` | Injury proneness modifier. |
| `mod_clutch` | `f64` | Clutch performance modifier. |
| `mod_hpp` | `f64` | Home-away splitting modifier. |
| `mod_platoon_splitting` | `f64` | Platoon splitting modifier. |

## Pitcher Base Skill Probability

These fields are intended to control skew when generating pitcher base skills.

| Parameter | Type |
| --- | --- |
| `velocity_skew` | `f64` |
| `control_skew` | `f64` |
| `stamina_skew` | `f64` |
| `injury_proneness_skew` | `f64` |
| `clutch_skew` | `f64` |
| `hpp_skew` | `f64` |
| `platoon_splitting_skew` | `f64` |

## Pitch Skill

`PitchSkill` represents pitch-specific modifiers.

| Parameter | Type | Description |
| --- | --- | --- |
| `pitch_type` | `PitchType` | Pitch type. |
| `mod_velocity` | `f64` | Pitch-specific velocity modifier. |
| `mod_control` | `f64` | Pitch-specific control modifier. |
| `mod_stamina` | `f64` | Pitch-specific stamina modifier. |
| `mod_injury_proneness` | `f64` | Pitch-specific injury proneness modifier. |
| `mod_stuff` | `f64` | Pitch quality modifier. |
| `mod_fb` | `f64` | Home run to fly ball rate modifier. |
| `mod_gp` | `f64` | Grounder percentage modifier. |
| `mod_horizontal_movement` | `f64` | Horizontal movement modifier. |
| `mod_vertical_movement` | `f64` | Vertical movement modifier. |
| `mod_spin_rate` | `f64` | Spin rate modifier. |

## Pitch Mix

| Parameter | Type | Description |
| --- | --- | --- |
| `pitch_type` | `PitchType` | Pitch type used in the mix. |
| `usage` | `u8` | Usage value for the pitch. |

