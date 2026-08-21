use crate::domain::resolver::batting_resolver::PlateApproach;
use crate::domain::shared::player::{BatterType, ZoneAptitude};
use crate::domain::shared::prob::ItemWeighted;
use crate::domain::strategy::pitching_strategy::TargetZone;
use crate::domain::util::GaussianPeak;
use strum_macros::AsRefStr;

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
