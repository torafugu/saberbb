use crate::domain::shared::prob::ItemWeighted;
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
