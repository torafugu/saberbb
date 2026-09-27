use crate::domain::shared::ball::{BallLocation, BallZone};
use crate::domain::shared::game_state::InningState;
use crate::domain::shared::player::PitchType;
use crate::domain::shared::prob::ItemWeighted;
use crate::domain::strategy::common_strategy::{
    DEFAULT_XBH_PROBS, HitAdvanceModel, RunExpectancyTable,
};
use crate::domain::util::sigmoid;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum_macros::{AsRefStr, EnumIter};

const WIDE_AIM_FACTOR: f64 = 3.0;
const EDGE_AIM_FACTOR: f64 = 4.0;
const OUT_AIM_FACTOR: f64 = -5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PitchingStrategy {
    AttackZone,
    HuntStrikeout,
    InduceGroundBall,
    AvoidExtraBases,
    PitchAround,
}

pub fn select_pitching_strategy(
    inning_state: &InningState,
    inning: u8,
    score_diff: i16, // If the score is a plus from the defensive team's perspective, it is a lead.
) -> PitchingStrategy {
    let mut strategy_score_map: HashMap<PitchingStrategy, u8> = HashMap::new();

    // Score by RunnerState and out count
    if inning_state.can_double_play() {
        strategy_score_map.insert(PitchingStrategy::InduceGroundBall, 5);
    } else if inning_state.in_play_score_chance() {
        strategy_score_map.insert(PitchingStrategy::HuntStrikeout, 5);
    }

    // Score by Pitcher ability
    if let Some(active_pitcher) = &inning_state.active_pitcher {
        if active_pitcher.pitcher.ground_ball_option() > 0.5 {
            strategy_score_map.insert(PitchingStrategy::InduceGroundBall, 2);
        }

        if active_pitcher.pitcher.whiff_option() > 0.5 {
            strategy_score_map.insert(PitchingStrategy::HuntStrikeout, 3);
        }

        if sigmoid(active_pitcher.pitcher.control) > 0.5 {
            strategy_score_map.insert(PitchingStrategy::AttackZone, 3);
        }
    }

    // Score by Batter ability
    if let Some(active_batter) = &inning_state.active_batter {
        if active_batter.batter.slugger_option() > 0.5 {
            strategy_score_map.insert(PitchingStrategy::AvoidExtraBases, 3);
        }

        if active_batter.batter.score() > 0.5 {
            strategy_score_map.insert(PitchingStrategy::PitchAround, 5);
        }
    }

    // TODO: Sumlated results should be used instead of MLB_2021_2024
    let re = RunExpectancyTable::MLB_2021_2024;
    if let Some(walk_cost_runs) = re.walk_cost_runs(&inning_state.runners, inning_state.out) {
        // TODO: walk_cost_runs should be updated based on PitchingStrategy distribution.
        if walk_cost_runs < 1.0 {
            strategy_score_map.insert(PitchingStrategy::PitchAround, 3);
        } else {
            strategy_score_map.insert(PitchingStrategy::AttackZone, 5);
        }
    }

    // TODO: Sumlated results should be used insyead of DEFAULT_ADVANCE_PROBS
    let advance = HitAdvanceModel::DEFAULT_ADVANCE_PROBS;

    if let Some(costs) = advance.costs(&re, &inning_state.runners, inning_state.out) {
        let extra_base_damage = DEFAULT_XBH_PROBS.double * (costs.double - costs.single)
            + DEFAULT_XBH_PROBS.triple * (costs.triple - costs.single)
            + DEFAULT_XBH_PROBS.home_run * (costs.home_run - costs.single);

        // TODO: extra_base_damage should be updated based on PitchingStrategy distribution.
        if extra_base_damage > 1.5 {
            strategy_score_map.insert(PitchingStrategy::AvoidExtraBases, 5);
        }
    }

    strategy_score_map
        .iter()
        .max_by_key(|(_, score)| *score)
        .map(|(strategy, _)| *strategy)
        .unwrap_or(PitchingStrategy::PitchAround)
}

pub struct PitchingPreferences {
    vertical: Option<TargetZonePreference>, // Inside / Outside / High / Low
    margin: Option<MarginPreference>,       // Wide / Edge / Out
    arsenal: Option<ArsenalPreference>,     // BestPitch / Mix
    sequence: Option<SequencePreference>,   // ChangeSpeeds / ChangeEyeLevel / ChangeSides
}

pub struct TargetZonePreference {
    zone: TargetZone,
    strength: f64,
}

pub struct MarginPreference {
    zone: Margin,
    strength: f64,
}

pub struct ArsenalPreference {
    zone: PitchArsenal,
    strength: f64,
}

pub struct SequencePreference {
    zone: PitchSequence,
    strength: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PitchArsenal {
    BestPitch,
    Mix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PitchSequence {
    ChangeSpeeds,
    ChangeEyeLevel,
    ChangeSides,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Hash, AsRefStr, Serialize, Deserialize)]
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
