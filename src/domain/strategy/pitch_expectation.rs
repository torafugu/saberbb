use crate::domain::shared::prob::ItemWeighted;

#[derive(Clone, Debug, PartialEq)]
pub enum BatterIntent {
    Swing,
    Take,
}

pub fn default_batter_intent() -> Vec<ItemWeighted<BatterIntent>> {
    let mut batter_intent = Vec::new();

    batter_intent.push(ItemWeighted {
        name: BatterIntent::Swing,
        weight: 0.8,
    });

    batter_intent.push(ItemWeighted {
        name: BatterIntent::Take,
        weight: 0.2,
    });

    batter_intent
}
