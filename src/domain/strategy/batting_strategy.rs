use crate::domain::resolver::batting_resolver::PlateApproach;
use crate::domain::shared::player::{BatterType, ZoneAptitude};
use crate::domain::shared::prob::ItemWeighted;
use crate::domain::strategy::pitching_strategy::TargetZone;
use crate::domain::util::GaussianPeak;
use strum_macros::AsRefStr;
use serde::{Deserialize, Serialize};

/// Explicit pre-swing intent. Selection from count/runner context is a separate layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BattedBallIntent {
    ContactFirst,
    LineDrive,
}

impl BattedBallIntent {
    pub fn aim_policy(self) -> &'static str {
        match self {
            Self::ContactFirst => "contact_center",
            Self::LineDrive => "line_drive_low_center",
        }
    }

    /// Frozen seed2 hypothesis: world-height cutoff, not normalized strike-zone height.
    pub fn aim_z_offset_m(self, forecast_height_m: f64) -> Result<f64, &'static str> {
        if !forecast_height_m.is_finite() {
            return Err("invalid_intent_forecast_height");
        }
        Ok(match self {
            Self::LineDrive if forecast_height_m >= 0.6 => -0.015,
            _ => 0.0,
        })
    }
}

/// Manager/coach objective. This layer never chooses a physical aim point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BattingStrategy {
    #[default]
    LetBatterDecide,
    PutBallInPlay,
    SeekLineDrive,
}

/// Explicit player-level preference; automatic character/count policies come later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BattingPreferences {
    pub preferred_intent: BattedBallIntent,
}

impl Default for BattingPreferences {
    fn default() -> Self { Self { preferred_intent: BattedBallIntent::ContactFirst } }
}

#[derive(Clone, Debug, Default)]
pub struct BattingStrategySettings {
    pub away_strategy: BattingStrategy,
    pub home_strategy: BattingStrategy,
    pub player_preferences: std::collections::BTreeMap<i64, BattingPreferences>,
}

impl BattingStrategySettings {
    pub fn decision(&self, is_top: bool, batter_id: i64) -> BattingIntentDecision {
        let strategy = if is_top { self.away_strategy } else { self.home_strategy };
        decide_batting_intent(strategy, self.player_preferences.get(&batter_id).copied().unwrap_or_default())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BattingDecisionReason { BatterPreference, ManagerContactPriority, ManagerLineDrivePriority }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BattingIntentDecision {
    pub strategy: BattingStrategy,
    pub preferences: BattingPreferences,
    pub selected_intent: BattedBallIntent,
    pub reason: BattingDecisionReason,
}

/// Resolve team objective and player preference before planning, with no result/RNG input.
pub fn decide_batting_intent(strategy: BattingStrategy, preferences: BattingPreferences) -> BattingIntentDecision {
    let (selected_intent, reason) = match strategy {
        BattingStrategy::LetBatterDecide => (preferences.preferred_intent, BattingDecisionReason::BatterPreference),
        BattingStrategy::PutBallInPlay => (BattedBallIntent::ContactFirst, BattingDecisionReason::ManagerContactPriority),
        BattingStrategy::SeekLineDrive => (BattedBallIntent::LineDrive, BattingDecisionReason::ManagerLineDrivePriority),
    };
    BattingIntentDecision { strategy, preferences, selected_intent, reason }
}

#[cfg(test)]
mod intent_levels_tests {
    use super::*;

    #[test]
    fn team_objective_and_player_preference_are_independently_recorded() {
        for intent in [BattedBallIntent::ContactFirst, BattedBallIntent::LineDrive] {
            let preferences = BattingPreferences { preferred_intent: intent };
            let free = decide_batting_intent(BattingStrategy::LetBatterDecide, preferences);
            assert_eq!(free.selected_intent, intent);
            assert_eq!(free.reason, BattingDecisionReason::BatterPreference);
            let contact = decide_batting_intent(BattingStrategy::PutBallInPlay, preferences);
            assert_eq!(contact.selected_intent, BattedBallIntent::ContactFirst);
            assert_eq!(contact.preferences, preferences);
            assert_eq!(contact.reason, BattingDecisionReason::ManagerContactPriority);
            let line = decide_batting_intent(BattingStrategy::SeekLineDrive, preferences);
            assert_eq!(line.selected_intent, BattedBallIntent::LineDrive);
            assert_eq!(line.preferences, preferences);
        }
    }

    #[test]
    fn settings_keep_home_away_and_individual_batter_preferences_separate() {
        let mut settings = BattingStrategySettings::default();
        settings.home_strategy = BattingStrategy::PutBallInPlay;
        settings.player_preferences.insert(42, BattingPreferences { preferred_intent: BattedBallIntent::LineDrive });
        assert_eq!(settings.decision(true, 42).selected_intent, BattedBallIntent::LineDrive);
        assert_eq!(settings.decision(true, 43).selected_intent, BattedBallIntent::ContactFirst);
        assert_eq!(settings.decision(false, 42).selected_intent, BattedBallIntent::ContactFirst);
    }
}

#[derive(Clone, Debug, PartialEq, AsRefStr)]
pub enum SwingExecution {
    Swing,
    Take,
}

pub fn default_batter_intent() -> Vec<ItemWeighted<SwingExecution>> {
    let mut batter_intent = Vec::new();

    batter_intent.push(ItemWeighted {
        name: SwingExecution::Swing,
        weight: 0.8,
    });

    batter_intent.push(ItemWeighted {
        name: SwingExecution::Take,
        weight: 0.2,
    });

    batter_intent
}

pub fn default_expected_zone() -> Vec<ItemWeighted<TargetZone>> {
    let mut expected_zone: Vec<ItemWeighted<TargetZone>> = Vec::new();

    expected_zone.push(ItemWeighted {
        name: TargetZone::Center,
        weight: 0.04,
    });

    expected_zone.push(ItemWeighted {
        name: TargetZone::LowOutside,
        weight: 0.48,
    });

    expected_zone.push(ItemWeighted {
        name: TargetZone::HighInside,
        weight: 0.24,
    });

    expected_zone.push(ItemWeighted {
        name: TargetZone::HighOutside,
        weight: 0.12,
    });

    expected_zone.push(ItemWeighted {
        name: TargetZone::LowInside,
        weight: 0.12,
    });

    expected_zone
}

pub fn default_plate_approach(batter_type: BatterType) -> Vec<ItemWeighted<PlateApproach>> {
    let mut plate_approach = Vec::new();
    match batter_type {
        BatterType::AggressiveFreeSwinger => {
            plate_approach.push(ItemWeighted {
                name: PlateApproach::Aggressive,
                weight: 0.7,
            });
            plate_approach.push(ItemWeighted {
                name: PlateApproach::Aggressive,
                weight: 0.2,
            });
            plate_approach.push(ItemWeighted {
                name: PlateApproach::Aggressive,
                weight: 0.1,
            });
        }
        BatterType::ClassicAnalyst => {
            plate_approach.push(ItemWeighted {
                name: PlateApproach::Aggressive,
                weight: 0.55,
            });
            plate_approach.push(ItemWeighted {
                name: PlateApproach::Aggressive,
                weight: 0.3,
            });
            plate_approach.push(ItemWeighted {
                name: PlateApproach::Aggressive,
                weight: 0.15,
            });
        }
        BatterType::GameManager => {
            plate_approach.push(ItemWeighted {
                name: PlateApproach::Aggressive,
                weight: 0.5,
            });
            plate_approach.push(ItemWeighted {
                name: PlateApproach::Aggressive,
                weight: 0.3,
            });
            plate_approach.push(ItemWeighted {
                name: PlateApproach::Aggressive,
                weight: 0.2,
            });
        }
        BatterType::ClutchHunter => {
            plate_approach.push(ItemWeighted {
                name: PlateApproach::Aggressive,
                weight: 0.6,
            });
            plate_approach.push(ItemWeighted {
                name: PlateApproach::Aggressive,
                weight: 0.3,
            });
            plate_approach.push(ItemWeighted {
                name: PlateApproach::Aggressive,
                weight: 0.1,
            });
        }
    }
    plate_approach
}

// TODO: Consider effects of situation.
pub fn calculate_attack_angle_modifier(batter_type: BatterType) -> f64 {
    match batter_type {
        BatterType::AggressiveFreeSwinger => 10.0,
        BatterType::ClassicAnalyst => 2.0,
        BatterType::GameManager => 3.0,
        BatterType::ClutchHunter => 0.0,
    }
}

pub fn zone_aptitude_peaks(zone_aptitude: ZoneAptitude) -> Vec<GaussianPeak> {
    match zone_aptitude {
        ZoneAptitude::Balanced => vec![GaussianPeak {
            center_x: 0.0,
            center_y: 0.0,
            amplitude: 0.100,
            sigma_x: 0.5,
            sigma_y: 0.5,
        }],
        ZoneAptitude::InsideDominant => vec![
            GaussianPeak {
                center_x: 0.0,
                center_y: 0.0,
                amplitude: 0.080,
                sigma_x: 0.5,
                sigma_y: 0.5,
            },
            GaussianPeak {
                center_x: -0.6,
                center_y: 0.0,
                amplitude: 0.060,
                sigma_x: 0.3,
                sigma_y: 0.4,
            },
        ],
        ZoneAptitude::OutsideDominant => vec![
            GaussianPeak {
                center_x: 0.0,
                center_y: 0.0,
                amplitude: 0.080,
                sigma_x: 0.5,
                sigma_y: 0.5,
            },
            GaussianPeak {
                center_x: 0.6,
                center_y: 0.0,
                amplitude: 0.060,
                sigma_x: 0.3,
                sigma_y: 0.4,
            },
        ],
        ZoneAptitude::HighBaller => vec![
            GaussianPeak {
                center_x: 0.0,
                center_y: 0.4,
                amplitude: 0.110,
                sigma_x: 0.5,
                sigma_y: 0.3,
            },
            GaussianPeak {
                center_x: 0.0,
                center_y: -0.6,
                amplitude: -0.050,
                sigma_x: 0.4,
                sigma_y: 0.2,
            },
        ],
        ZoneAptitude::LowBaller => vec![
            GaussianPeak {
                center_x: 0.0,
                center_y: -0.4,
                amplitude: 0.110,
                sigma_x: 0.5,
                sigma_y: 0.3,
            },
            GaussianPeak {
                center_x: 0.0,
                center_y: 0.6,
                amplitude: -0.050,
                sigma_x: 0.4,
                sigma_y: 0.2,
            },
        ],
        ZoneAptitude::DiagonalCross => vec![
            GaussianPeak {
                center_x: 0.4,
                center_y: 0.4,
                amplitude: 0.060,
                sigma_x: 0.5,
                sigma_y: 0.3,
            },
            GaussianPeak {
                center_x: -0.4,
                center_y: -0.4,
                amplitude: 0.060,
                sigma_x: 0.5,
                sigma_y: 0.3,
            },
            GaussianPeak {
                center_x: 0.6,
                center_y: -0.6,
                amplitude: -0.050,
                sigma_x: 0.4,
                sigma_y: 0.2,
            },
            GaussianPeak {
                center_x: -0.6,
                center_y: 0.6,
                amplitude: -0.050,
                sigma_x: 0.4,
                sigma_y: 0.2,
            },
        ],
    }
}
