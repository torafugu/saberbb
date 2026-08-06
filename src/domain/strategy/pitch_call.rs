use crate::domain::shared::ball::BallLocation;
use crate::domain::shared::ball::Zone;
use crate::domain::shared::player::PitchType;
use crate::domain::shared::prob::ItemWeighted;
use strum_macros::AsRefStr;

const WIDE_AIM_FACTOR: f64 = 3.0;
const EDGE_AIM_FACTOR: f64 = 4.0;
const OUT_AIM_FACTOR: f64 = -5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Margin {
    Wide,
    Edge,
    Out,
}
impl Margin {
    pub fn factor(&self) -> f64 {
        match self {
            Margin::Wide => WIDE_AIM_FACTOR,
            Margin::Edge => EDGE_AIM_FACTOR,
            Margin::Out => OUT_AIM_FACTOR,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, AsRefStr)]
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
    pub margin: Margin,
}
impl PitchCall {
    pub fn aim_location(&self) -> BallLocation {
        match self.target_location {
            TargetLocation::Center => BallLocation { x: 0.0, y: 0.0 },
            TargetLocation::LowInside => BallLocation {
                x: self.target_location.zone().x1
                    + self.target_location.zone().width() / self.margin.factor(),
                y: self.target_location.zone().y2
                    + self.target_location.zone().height() / self.margin.factor(),
            },
            TargetLocation::LowOutside => BallLocation {
                x: self.target_location.zone().x2
                    - self.target_location.zone().width() / self.margin.factor(),
                y: self.target_location.zone().y2
                    + self.target_location.zone().height() / self.margin.factor(),
            },
            TargetLocation::HighInside => BallLocation {
                x: self.target_location.zone().x1
                    + self.target_location.zone().width() / self.margin.factor(),
                y: self.target_location.zone().y1
                    - self.target_location.zone().height() / self.margin.factor(),
            },
            TargetLocation::HighOutside => BallLocation {
                x: self.target_location.zone().x2
                    - self.target_location.zone().width() / self.margin.factor(),
                y: self.target_location.zone().y1
                    - self.target_location.zone().height() / self.margin.factor(),
            },
        }
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
