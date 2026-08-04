use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::ball::BallLocation;
use crate::domain::shared::ball::Zone;
use crate::domain::shared::player::PitchType;
use crate::domain::shared::prob::ItemWeighted;

/// Target Course (狙い目のエリア)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetLocation {
    Center,
    LowInside,
    LowOutside,
    HighInside,
    HighOutside,
}
impl TargetLocation {
    pub fn zone(self) -> Zone {
        match self {
            TargetLocation::Center => Zone {
                x1: -0.25,
                y1: 0.25,
                x2: 0.25,
                y2: -0.25,
            },
            TargetLocation::LowInside => Zone {
                x1: -1.0,
                y1: 0.0,
                x2: 0.0,
                y2: -1.0,
            },
            TargetLocation::LowOutside => Zone {
                x1: 0.0,
                y1: 0.0,
                x2: 1.0,
                y2: -1.0,
            },
            TargetLocation::HighInside => Zone {
                x1: -1.0,
                y1: 1.0,
                x2: 0.0,
                y2: 0.0,
            },
            TargetLocation::HighOutside => Zone {
                x1: 0.0,
                y1: 1.0,
                x2: 1.0,
                y2: 0.0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PitchCall {
    pub pitch_type: PitchType,
    pub target_location: TargetLocation,
}
impl PitchCall {
    pub fn random_ball_location(&self, rng: &mut dyn RandomProvider) -> BallLocation {
        self.target_location.zone().random_ball_location(rng)
    }
}

pub fn default_location_distribution() -> Vec<ItemWeighted<TargetLocation>> {
    let mut locations = Vec::new();

    locations.push(ItemWeighted {
        name: TargetLocation::LowOutside,
        weight: 0.8,
    });

    locations.push(ItemWeighted {
        name: TargetLocation::HighInside,
        weight: 0.2,
    });

    locations
}
