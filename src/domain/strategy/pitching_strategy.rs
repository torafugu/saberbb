use crate::domain::shared::ball::BallLocation;
use crate::domain::shared::ball::BallZone;
use crate::domain::shared::player::PitchType;
use crate::domain::shared::prob::ItemWeighted;
use strum_macros::{AsRefStr, EnumIter};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Hash, AsRefStr)]
pub enum TargetZoneSimilarity {
    Same,
    Height,
    Course,
    Opposite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Hash, AsRefStr)]
pub enum TargetZone {
    Center,
    LowInside,
    LowOutside,
    HighInside,
    HighOutside,
}
impl TargetZone {
    pub fn zone(self) -> BallZone {
        match self {
            TargetZone::Center => BallZone {
                x1: -0.25,
                y1: 0.25,
                x2: 0.25,
                y2: -0.25,
            },
            TargetZone::LowInside => BallZone {
                x1: -1.0,
                y1: 0.0,
                x2: 0.0,
                y2: -1.0,
            },
            TargetZone::LowOutside => BallZone {
                x1: 0.0,
                y1: 0.0,
                x2: 1.0,
                y2: -1.0,
            },
            TargetZone::HighInside => BallZone {
                x1: -1.0,
                y1: 1.0,
                x2: 0.0,
                y2: 0.0,
            },
            TargetZone::HighOutside => BallZone {
                x1: 0.0,
                y1: 1.0,
                x2: 1.0,
                y2: 0.0,
            },
        }
    }

    pub fn similarity(self, another: TargetZone) -> TargetZoneSimilarity {
        let is_same_hight = self.zone().x1 == another.zone().x1;
        let is_same_course = self.zone().y1 == another.zone().y1;

        match (is_same_hight, is_same_course) {
            (true, true) => TargetZoneSimilarity::Same,
            (true, false) => TargetZoneSimilarity::Height,
            (false, true) => TargetZoneSimilarity::Course,
            (false, false) => TargetZoneSimilarity::Opposite,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PitchCall {
    pub pitch_type: PitchType,
    pub target_zone: TargetZone,
    pub margin: Margin,
}
impl PitchCall {
    pub fn aim_location(&self) -> BallLocation {
        match self.target_zone {
            TargetZone::Center => BallLocation { x: 0.0, y: 0.0 },
            TargetZone::LowInside => BallLocation {
                x: self.target_zone.zone().x1
                    + self.target_zone.zone().width() / self.margin.factor(),
                y: self.target_zone.zone().y2
                    + self.target_zone.zone().height() / self.margin.factor(),
            },
            TargetZone::LowOutside => BallLocation {
                x: self.target_zone.zone().x2
                    - self.target_zone.zone().width() / self.margin.factor(),
                y: self.target_zone.zone().y2
                    + self.target_zone.zone().height() / self.margin.factor(),
            },
            TargetZone::HighInside => BallLocation {
                x: self.target_zone.zone().x1
                    + self.target_zone.zone().width() / self.margin.factor(),
                y: self.target_zone.zone().y1
                    - self.target_zone.zone().height() / self.margin.factor(),
            },
            TargetZone::HighOutside => BallLocation {
                x: self.target_zone.zone().x2
                    - self.target_zone.zone().width() / self.margin.factor(),
                y: self.target_zone.zone().y1
                    - self.target_zone.zone().height() / self.margin.factor(),
            },
        }
    }
}

pub fn default_location_distribution() -> Vec<ItemWeighted<TargetZone>> {
    let mut locations = Vec::new();

    locations.push(ItemWeighted {
        name: TargetZone::LowOutside,
        weight: 0.8,
    });

    locations.push(ItemWeighted {
        name: TargetZone::HighInside,
        weight: 0.2,
    });

    locations
}
