use crate::domain::shared::ball::{BallLocation, BallZone};
use crate::domain::shared::game_state::InningState;
use crate::domain::shared::player::{BatterInfo, PitchType, PitcherInfo, PitcherTendencies};
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

pub struct PitchTypeEstimate {
    pub whiff: f64,        // Estimated ability to induce swings and misses: 0.0–1.0
    pub ground_ball: f64,  // Estimated ability to induce ground balls
    pub hard_contact: f64, // Estimated risk of allowing hard contact
    pub command: f64,      // Estimated ability to hit the intended location
}

pub fn estimate_pitch_types(
    pitcher: &PitcherInfo,
    batter: &BatterInfo,
) -> HashMap<PitchType, PitchTypeEstimate> {
    pitcher
        .pitch_skills
        .iter()
        .map(|skill| {
            // TODO: base_whiff, base_ground_ball, base_hard_contact should be updated based on pitching simulation result.
            // Provisional baseline values by pitch type. All are selection scores from 0.0 to 1.0.
            let (base_whiff, base_ground_ball, base_hard_contact) = match skill.pitch_type {
                PitchType::FourSeamFastball => (0.55, 0.35, 0.50),
                PitchType::Cutter => (0.50, 0.50, 0.42),
                PitchType::Curveball => (0.55, 0.50, 0.38),
                PitchType::Slider => (0.65, 0.42, 0.38),
                PitchType::Changeup => (0.55, 0.58, 0.40),
                PitchType::Splitter => (0.65, 0.65, 0.35),
            };

            let estimate = PitchTypeEstimate {
                whiff: (base_whiff + 0.15 * (pitcher.whiff_option() - 0.5)
                    - 0.10 * (batter.bat_control - 0.5))
                    .clamp(0.0, 1.0),

                ground_ball: (base_ground_ball + 0.15 * (pitcher.ground_ball_option() - 0.5))
                    .clamp(0.0, 1.0),

                hard_contact: (base_hard_contact + 0.15 * (batter.slugger_option() - 0.5))
                    .clamp(0.0, 1.0),

                command: (0.5
                    + 0.25 * (sigmoid(pitcher.control) - 0.5)
                    + 0.25 * (sigmoid(skill.control) - 0.5))
                    .clamp(0.0, 1.0),
            };

            (skill.pitch_type, estimate)
        })
        .collect()
}

pub struct PitchTypeProposal {
    pub pitch_type: PitchType,
    pub score: f64,
    pub estimate: PitchTypeEstimate,
}

pub fn shortlist_pitch_types(
    pitcher: &PitcherInfo,
    tendencies: &PitcherTendencies,
    strategy: PitchingStrategy,
    previous_pitch: Option<PitchType>,
    estimates: &HashMap<PitchType, PitchTypeEstimate>,
    limit: usize,
) -> Vec<PitchTypeProposal> {
    let mut proposals: Vec<_> = pitcher
        .pitch_skill_distribution()
        .into_iter()
        .filter_map(|item| {
            let pitch_type = item.name.pitch_type;
            let estimate = estimates.get(&pitch_type)?;

            // Base usage rate derived from the existing PitchSkill.usage.
            // Convert to log scale to match the score adjustments below.
            let usage_score = item.weight.max(1e-9).ln();

            let strategy_score = match strategy {
                PitchingStrategy::AttackZone => 0.8 * estimate.command,
                PitchingStrategy::HuntStrikeout => 0.8 * estimate.whiff,
                PitchingStrategy::InduceGroundBall => 0.8 * estimate.ground_ball,
                PitchingStrategy::AvoidExtraBases => -0.8 * estimate.hard_contact,
                PitchingStrategy::PitchAround => -0.4 * estimate.hard_contact,
            };

            let tendency_score = -0.6 * tendencies.hard_contact_aversion * estimate.hard_contact
                - 0.6 * tendencies.walk_aversion * (1.0 - estimate.command)
                + 0.3 * tendencies.pitch_variety * f64::from(previous_pitch != Some(pitch_type));

            Some(PitchTypeProposal {
                pitch_type,
                score: usage_score + strategy_score + tendency_score,
                estimate: PitchTypeEstimate {
                    whiff: estimate.whiff,
                    ground_ball: estimate.ground_ball,
                    hard_contact: estimate.hard_contact,
                    command: estimate.command,
                },
            })
        })
        .collect();

    // sort_by is stable, preserving the original pitch_skills order for ties.
    proposals.sort_by(|a, b| b.score.total_cmp(&a.score));
    proposals.truncate(limit);
    proposals
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PitchingStrategy {
    AttackZone,
    HuntStrikeout,
    InduceGroundBall,
    AvoidExtraBases,
    PitchAround,
}

const STRATEGY_PRIORITY: [PitchingStrategy; 5] = [
    PitchingStrategy::AttackZone,
    PitchingStrategy::HuntStrikeout,
    PitchingStrategy::InduceGroundBall,
    PitchingStrategy::AvoidExtraBases,
    PitchingStrategy::PitchAround,
];

pub fn select_pitching_strategy(
    inning_state: &InningState,
    inning: u8,
    score_diff: i16, // If the score is a plus from the defensive team's perspective, it is a lead.
) -> PitchingStrategy {
    let mut strategy_score_map: HashMap<PitchingStrategy, i32> = HashMap::new();

    // Score by RunnerState and out count
    if inning_state.can_double_play() {
        add_score(
            &mut strategy_score_map,
            PitchingStrategy::InduceGroundBall,
            5,
        );
    } else if inning_state.in_play_score_chance() {
        add_score(&mut strategy_score_map, PitchingStrategy::HuntStrikeout, 5);
    }

    // Score by Pitcher ability
    if let Some(active_pitcher) = &inning_state.active_pitcher {
        if active_pitcher.pitcher.ground_ball_option() > 0.5 {
            add_score(
                &mut strategy_score_map,
                PitchingStrategy::InduceGroundBall,
                2,
            );
        }

        if active_pitcher.pitcher.whiff_option() > 0.5 {
            add_score(&mut strategy_score_map, PitchingStrategy::HuntStrikeout, 3);
        }

        if sigmoid(active_pitcher.pitcher.control) > 0.5 {
            add_score(&mut strategy_score_map, PitchingStrategy::AttackZone, 3);
        }
    }

    // Score by Batter ability
    if let Some(active_batter) = &inning_state.active_batter {
        if active_batter.batter.slugger_option() > 0.5 {
            add_score(
                &mut strategy_score_map,
                PitchingStrategy::AvoidExtraBases,
                3,
            );
        }

        if active_batter.batter.score() > 0.5 {
            add_score(&mut strategy_score_map, PitchingStrategy::PitchAround, 5);
        }
    }

    // TODO: Sumlated results should be used instead of MLB_2021_2024
    let re = RunExpectancyTable::MLB_2021_2024;
    if let Some(walk_cost_runs) = re.walk_cost_runs(&inning_state.runners, inning_state.out) {
        // TODO: walk_cost_runs should be updated based on PitchingStrategy distribution.
        if walk_cost_runs < 1.0 {
            add_score(&mut strategy_score_map, PitchingStrategy::PitchAround, 3);
        } else {
            add_score(&mut strategy_score_map, PitchingStrategy::AttackZone, 5);
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
            add_score(
                &mut strategy_score_map,
                PitchingStrategy::AvoidExtraBases,
                5,
            );
        }
    }

    STRATEGY_PRIORITY
        .into_iter()
        .enumerate()
        .max_by_key(|(index, strategy)| {
            (
                strategy_score_map.get(strategy).copied().unwrap_or(0),
                std::cmp::Reverse(*index),
            )
        })
        .map(|(_, strategy)| strategy)
        .unwrap_or(PitchingStrategy::AttackZone)
}

fn add_score(scores: &mut HashMap<PitchingStrategy, i32>, strategy: PitchingStrategy, points: i32) {
    *scores.entry(strategy).or_insert(0) += points;
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
