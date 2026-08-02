use crate::domain::shared::ball::LocationZone;
use crate::domain::shared::player::PitchType;
use crate::domain::shared::prob::ItemWeighted;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PitchCall {
    pub pitch_type: PitchType,
    pub location: LocationZone,
}

pub fn default_location_distribution() -> Vec<ItemWeighted<LocationZone>> {
    let mut locations = Vec::new();

    locations.push(ItemWeighted {
        name: LocationZone::LowAway,
        weight: 0.8,
    });

    locations.push(ItemWeighted {
        name: LocationZone::UpIn,
        weight: 0.2,
    });

    locations
}
