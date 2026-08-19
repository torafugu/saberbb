use crate::domain::shared::prob::ItemWeighted;
use crate::domain::strategy::pitching_strategy::TargetZone;
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
