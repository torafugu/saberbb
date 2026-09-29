use crate::domain::resolver::batting_resolver::CountStatus;
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

struct PitchTypeEstimate {
    pub whiff: f64,        // Estimated ability to induce swings and misses: 0.0–1.0
    pub ground_ball: f64,  // Estimated ability to induce ground balls
    pub hard_contact: f64, // Estimated risk of allowing hard contact
    pub command: f64,      // Estimated ability to hit the intended location
}

fn estimate_pitch_types(
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

struct PitchTypeProposal {
    pub pitch_type: PitchType,
    pub score: f64,
    pub estimate: PitchTypeEstimate,
}

fn pitch_type_strategy_score(strategy: PitchingStrategy, estimate: &PitchTypeEstimate) -> f64 {
    match strategy {
        PitchingStrategy::AttackZone => 0.8 * estimate.command,
        PitchingStrategy::HuntStrikeout => 0.8 * estimate.whiff,
        PitchingStrategy::InduceGroundBall => 0.8 * estimate.ground_ball,
        PitchingStrategy::AvoidExtraBases => -0.8 * estimate.hard_contact,
        PitchingStrategy::PitchAround => -0.4 * estimate.hard_contact,
    }
}

fn shortlist_pitch_types(
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

            let strategy_score = pitch_type_strategy_score(strategy, estimate);

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

/// Relative risks used for ranking calls, not observed event probabilities.
#[derive(Debug, Clone, Copy)]
pub struct PitchRisks {
    pub walk: f64,
    pub hard_contact: f64,
    pub execution: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct PitchCallProposal {
    pub pitch_call: PitchCall,
    pub score: f64,
    pub risks: PitchRisks,
}

fn estimate_call_risks(
    pitch_call: PitchCall,
    estimate: &PitchTypeEstimate,
    batter: &BatterInfo,
) -> PitchRisks {
    let aim = pitch_call.aim_location();
    let edge = aim.x.abs().max(aim.y.abs());
    let outside = (edge - 1.0).max(0.0);
    let batter_aptitude = batter.zone_modifier(&aim).clamp(-0.2, 0.5);

    // Provisional indices; a ball at this location is not a walk without
    // considering the count and the rest of the plate appearance.
    PitchRisks {
        walk: (0.10 + 0.25 * (1.0 - estimate.command) + 0.15 * edge + 0.8 * outside)
            .clamp(0.0, 1.0),
        hard_contact: (estimate.hard_contact
            * (0.8 + 2.0 * batter_aptitude)
            * (1.0 - 0.15 * edge.min(1.0)))
            .clamp(0.0, 1.0),
        execution: ((1.0 - estimate.command) * (0.3 + 0.3 * edge)).clamp(0.0, 1.0),
    }
}

fn pitch_call_strategy_score(
    strategy: PitchingStrategy,
    pitch_call: PitchCall,
    estimate: &PitchTypeEstimate,
    risks: PitchRisks,
) -> f64 {
    let aim = pitch_call.aim_location();
    let edge = aim.x.abs().max(aim.y.abs());
    let outside = (edge - 1.0).max(0.0);

    match strategy {
        PitchingStrategy::AttackZone => -0.8 * risks.walk,
        PitchingStrategy::HuntStrikeout => {
            0.3 * estimate.whiff * edge.min(1.0) - 0.3 * risks.walk
        }
        PitchingStrategy::InduceGroundBall => {
            0.4 * estimate.ground_ball * f64::from(aim.y < 0.0) - 0.3 * risks.walk
        }
        PitchingStrategy::AvoidExtraBases => -0.8 * risks.hard_contact,
        PitchingStrategy::PitchAround => -0.8 * risks.hard_contact + 0.3 * outside,
    }
}

/// Generate the pitcher's proposals from one strategy and one set of tendencies.
/// Keep a separate location shortlist for each pitch type.
pub fn shortlist_pitch_calls(
    pitcher: &PitcherInfo,
    batter: &BatterInfo,
    strategy: PitchingStrategy,
    previous_pitch: Option<PitchType>,
    pitch_type_limit: usize,
    limit_per_pitch: usize,
) -> Vec<PitchCallProposal> {
    let tendencies = pitcher.pitcher_character.tendencies();
    let estimates = estimate_pitch_types(pitcher, batter);
    let pitch_types = shortlist_pitch_types(
        pitcher,
        &tendencies,
        strategy,
        previous_pitch,
        &estimates,
        pitch_type_limit,
    );

    shortlist_locations_for_pitch_types(
        &pitch_types,
        batter,
        &tendencies,
        strategy,
        limit_per_pitch,
    )
}

fn shortlist_locations_for_pitch_types(
    pitch_types: &[PitchTypeProposal],
    batter: &BatterInfo,
    tendencies: &PitcherTendencies,
    strategy: PitchingStrategy,
    limit_per_pitch: usize,
) -> Vec<PitchCallProposal> {
    const ZONES: [TargetZone; 5] = [
        TargetZone::Center,
        TargetZone::LowInside,
        TargetZone::LowOutside,
        TargetZone::HighInside,
        TargetZone::HighOutside,
    ];
    const MARGINS: [Margin; 3] = [Margin::Wide, Margin::Edge, Margin::Out];

    let mut shortlisted = Vec::new();
    for pitch in pitch_types {
        let mut proposals = Vec::with_capacity(13); // Center once + four corners × three margins
        for zone in ZONES {
            for margin in MARGINS {
                // Margin does not affect the aim location of a center pitch.
                if zone == TargetZone::Center && margin != Margin::Wide {
                    continue;
                }

                let pitch_call = PitchCall {
                    pitch_type: pitch.pitch_type,
                    target_zone: zone,
                    margin,
                };
                let risks = estimate_call_risks(pitch_call, &pitch.estimate, batter);
                let strategy_score =
                    pitch_call_strategy_score(strategy, pitch_call, &pitch.estimate, risks);
                let tendency_score = -0.5 * tendencies.walk_aversion * risks.walk
                    - 0.5 * tendencies.hard_contact_aversion * risks.hard_contact;

                proposals.push(PitchCallProposal {
                    pitch_call,
                    score: pitch.score + strategy_score + tendency_score - 0.3 * risks.execution,
                    risks,
                });
            }
        }

        // Stable sorting also gives deterministic tie handling within each pitch.
        proposals.sort_by(|a, b| b.score.total_cmp(&a.score));
        proposals.truncate(limit_per_pitch);
        shortlisted.extend(proposals);
    }
    shortlisted
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

#[derive(Debug, Clone, Copy, Default)]
pub struct PitchingPreferences {
    pub target_zone: Option<TargetZonePreference>,
    pub margin: Option<MarginPreference>,
    pub arsenal: Option<ArsenalPreference>,
    pub sequence: Option<SequencePreference>,
}

#[derive(Debug, Clone, Copy)]
pub struct TargetZonePreference {
    pub zone: TargetZone,
    pub strength: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct MarginPreference {
    pub margin: Margin,
    pub strength: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct ArsenalPreference {
    pub arsenal: PitchArsenal,
    pub strength: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct SequencePreference {
    pub sequence: PitchSequence,
    pub strength: f64,
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

/// Calling style supplied by the caller, independent of the pitcher's character.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CatcherCallingStyle {
    #[default]
    Balanced,
    Aggressive,
    Cautious,
    Adaptive,
}

/// Express the catcher's wishes without selecting a pitch or estimating risk.
/// Strategy and pitcher feasibility are evaluated only when producing proposals.
/// All strengths are provisional, soft scoring weights in the range 0.0..=1.0.
pub fn catcher_preferences(
    style: CatcherCallingStyle,
    batter: &BatterInfo,
    count: CountStatus,
    previous_call: Option<PitchCall>,
) -> PitchingPreferences {
    let (margin, margin_strength, arsenal, arsenal_strength, sequence, sequence_strength) =
        match style {
            CatcherCallingStyle::Balanced => (
                Margin::Edge,
                0.35,
                PitchArsenal::Mix,
                0.35,
                PitchSequence::ChangeSpeeds,
                0.35,
            ),
            CatcherCallingStyle::Aggressive => (
                Margin::Wide,
                0.65,
                PitchArsenal::BestPitch,
                0.7,
                PitchSequence::ChangeEyeLevel,
                0.4,
            ),
            CatcherCallingStyle::Cautious => (
                Margin::Edge,
                0.75,
                PitchArsenal::Mix,
                0.5,
                PitchSequence::ChangeSides,
                0.5,
            ),
            CatcherCallingStyle::Adaptive => (
                Margin::Edge,
                0.4,
                PitchArsenal::Mix,
                0.6,
                PitchSequence::ChangeSpeeds,
                0.7,
            ),
        };
    let mut preferences = PitchingPreferences {
        target_zone: catcher_target_zone_preference(batter),
        margin: Some(MarginPreference {
            margin,
            strength: margin_strength,
        }),
        arsenal: Some(ArsenalPreference {
            arsenal,
            strength: arsenal_strength,
        }),
        sequence: previous_call.map(|last| {
            let sequence = if style == CatcherCallingStyle::Adaptive {
                let aim = last.aim_location();
                if aim.y > 0.0 {
                    PitchSequence::ChangeEyeLevel
                } else if aim.x != 0.0 {
                    PitchSequence::ChangeSides
                } else {
                    PitchSequence::ChangeSpeeds
                }
            } else {
                sequence
            };
            SequencePreference {
                sequence,
                strength: sequence_strength,
            }
        }),
    };

    // Three balls take precedence over two strikes, including a full count.
    // These are count-based wishes; PitchAround can still favor Out in proposals.
    if matches!(count, CountStatus::C30 | CountStatus::C31 | CountStatus::C32) {
        preferences.margin = Some(MarginPreference {
            margin: Margin::Wide,
            strength: 0.9,
        });
        preferences.arsenal = Some(ArsenalPreference {
            arsenal: PitchArsenal::BestPitch,
            strength: 0.8,
        });
    } else if count.is_strike_two() {
        preferences.margin = Some(MarginPreference {
            margin: Margin::Edge,
            strength: 0.7,
        });
        preferences.arsenal = Some(ArsenalPreference {
            arsenal: PitchArsenal::Mix,
            strength: 0.65,
        });
    }
    // Mixing pitches needs a previous pitch to compare against.
    if previous_call.is_none()
        && preferences
            .arsenal
            .is_some_and(|p| p.arsenal == PitchArsenal::Mix)
    {
        preferences.arsenal = None;
    }
    preferences
}

fn catcher_target_zone_preference(batter: &BatterInfo) -> Option<TargetZonePreference> {
    // Compare corners at a fixed margin so location wishes do not encode margin.
    const ZONES: [TargetZone; 4] = [
        TargetZone::LowOutside,
        TargetZone::LowInside,
        TargetZone::HighOutside,
        TargetZone::HighInside,
    ];
    let aptitudes = ZONES.map(|zone| {
        let aim = PitchCall {
            pitch_type: PitchType::FourSeamFastball,
            target_zone: zone,
            margin: Margin::Edge,
        }
        .aim_location();
        (zone, batter.zone_modifier(&aim))
    });
    let &(zone, weakest) = aptitudes.iter().min_by(|a, b| a.1.total_cmp(&b.1))?;
    let strongest = aptitudes
        .iter()
        .map(|(_, value)| *value)
        .fold(f64::NEG_INFINITY, f64::max);
    let spread = strongest - weakest;
    if spread <= 1e-9 {
        return None; // Equal aptitude should not invent a preferred corner.
    }
    Some(TargetZonePreference {
        zone,
        strength: (spread / 0.2).clamp(0.0, 0.7),
    })
}

/// Generate the catcher's proposals independently of the pitcher's shortlist.
/// Preferences express the catcher's desired call; the pitcher and batter
/// supply feasibility and risk estimates for the same PitchCallProposal type.
pub fn catcher_pitch_call_proposals(
    preferences: &PitchingPreferences,
    pitcher: &PitcherInfo,
    batter: &BatterInfo,
    strategy: PitchingStrategy,
    previous_call: Option<PitchCall>,
    limit: usize,
) -> Vec<PitchCallProposal> {
    const ZONES: [TargetZone; 5] = [
        TargetZone::Center,
        TargetZone::LowInside,
        TargetZone::LowOutside,
        TargetZone::HighInside,
        TargetZone::HighOutside,
    ];
    const MARGINS: [Margin; 3] = [Margin::Wide, Margin::Edge, Margin::Out];

    let estimates = estimate_pitch_types(pitcher, batter);
    let previous_skill = previous_call.and_then(|call| {
        pitcher
            .pitch_skills
            .iter()
            .find(|skill| skill.pitch_type == call.pitch_type)
    });
    let mut proposals = Vec::new();
    for item in pitcher.pitch_skill_distribution() {
        let pitch_type = item.name.pitch_type;
        let Some(estimate) = estimates.get(&pitch_type) else {
            continue;
        };

        for zone in ZONES {
            for margin in MARGINS {
                if zone == TargetZone::Center && margin != Margin::Wide {
                    continue;
                }
                let pitch_call = PitchCall {
                    pitch_type,
                    target_zone: zone,
                    margin,
                };
                let risks = estimate_call_risks(pitch_call, estimate, batter);
                let mut score = item.weight.max(1e-9).ln()
                    + pitch_type_strategy_score(strategy, estimate)
                    + pitch_call_strategy_score(strategy, pitch_call, estimate, risks)
                    - 0.15 * risks.walk
                    - 0.25 * risks.hard_contact
                    - 0.15 * risks.execution;

                if let Some(preferred) = preferences.target_zone {
                    if zone == preferred.zone {
                        score += 0.8 * preferred.strength.clamp(0.0, 1.0);
                    }
                }
                if let Some(preferred) = preferences.margin {
                    if margin == preferred.margin {
                        score += 0.5 * preferred.strength.clamp(0.0, 1.0);
                    }
                }
                if let Some(preferred) = preferences.arsenal {
                    let bonus = match preferred.arsenal {
                        PitchArsenal::BestPitch => item.weight,
                        PitchArsenal::Mix => {
                            f64::from(previous_call.is_some_and(|call| call.pitch_type != pitch_type))
                        }
                    };
                    score += 0.3 * preferred.strength.clamp(0.0, 1.0) * bonus;
                }
                if let (Some(preferred), Some(last_call)) = (preferences.sequence, previous_call) {
                    let last_aim = last_call.aim_location();
                    let aim = pitch_call.aim_location();
                    let bonus = match preferred.sequence {
                        PitchSequence::ChangeSpeeds => previous_skill.map_or(0.0, |last_skill| {
                            let a = item.name.velocity;
                            let b = last_skill.velocity;
                            (a - b).abs() / a.abs().max(b.abs()).max(1e-9)
                        }),
                        PitchSequence::ChangeEyeLevel => {
                            f64::from(aim.y * last_aim.y < 0.0)
                        }
                        PitchSequence::ChangeSides => f64::from(aim.x * last_aim.x < 0.0),
                    };
                    score += 0.3 * preferred.strength.clamp(0.0, 1.0) * bonus;
                }

                proposals.push(PitchCallProposal {
                    pitch_call,
                    score,
                    risks,
                });
            }
        }
    }

    proposals.sort_by(|a, b| b.score.total_cmp(&a.score));
    proposals.truncate(limit);
    proposals
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

#[cfg(test)]
mod pitch_call_shortlist_tests {
    use super::*;
    use crate::domain::shared::player::{RL, ZoneAptitude};
    use crate::domain::test_support::{batter_info, pitcher_info};

    #[test]
    fn catcher_count_wishes_override_style_and_full_count_prioritizes_walks() {
        let batter = batter_info(RL::Right);
        let previous = Some(PitchCall {
            pitch_type: PitchType::FourSeamFastball,
            target_zone: TargetZone::HighInside,
            margin: Margin::Edge,
        });
        for style in [
            CatcherCallingStyle::Balanced,
            CatcherCallingStyle::Aggressive,
            CatcherCallingStyle::Cautious,
            CatcherCallingStyle::Adaptive,
        ] {
            for count in [CountStatus::C30, CountStatus::C31, CountStatus::C32] {
                let preferences = catcher_preferences(style, &batter, count, previous);
                assert_eq!(preferences.margin.unwrap().margin, Margin::Wide);
                assert_eq!(preferences.arsenal.unwrap().arsenal, PitchArsenal::BestPitch);
            }
            for count in [CountStatus::C02, CountStatus::C12, CountStatus::C22] {
                let preferences = catcher_preferences(style, &batter, count, previous);
                assert_eq!(preferences.margin.unwrap().margin, Margin::Edge);
                assert_eq!(preferences.arsenal.unwrap().arsenal, PitchArsenal::Mix);
            }
        }
    }

    #[test]
    fn catcher_zone_wishes_follow_batter_aptitude_without_arbitrary_balanced_bias() {
        let mut batter = batter_info(RL::Right);
        batter.zone_aptitude = ZoneAptitude::Balanced;
        assert!(catcher_preferences(
            CatcherCallingStyle::Balanced, &batter, CountStatus::C00, None,
        ).target_zone.is_none());

        batter.zone_aptitude = ZoneAptitude::InsideDominant;
        let preference = catcher_target_zone_preference(&batter).unwrap();
        assert!(matches!(preference.zone, TargetZone::LowOutside | TargetZone::HighOutside));
        assert!(preference.strength > 0.0 && preference.strength <= 0.7);

        batter.zone_aptitude = ZoneAptitude::LowBaller;
        let preference = catcher_target_zone_preference(&batter).unwrap();
        assert!(matches!(preference.zone, TargetZone::HighInside | TargetZone::HighOutside));
    }

    #[test]
    fn catcher_style_and_previous_call_control_sequence_wishes() {
        let batter = batter_info(RL::Right);
        let initial = catcher_preferences(
            CatcherCallingStyle::Adaptive, &batter, CountStatus::C00, None,
        );
        assert!(initial.sequence.is_none());
        assert!(initial.arsenal.is_none());
        let aggressive = catcher_preferences(
            CatcherCallingStyle::Aggressive, &batter, CountStatus::C00, None,
        );
        let cautious = catcher_preferences(
            CatcherCallingStyle::Cautious, &batter, CountStatus::C00, None,
        );
        assert_eq!(aggressive.margin.unwrap().margin, Margin::Wide);
        assert_eq!(cautious.margin.unwrap().margin, Margin::Edge);

        for (zone, expected) in [
            (TargetZone::HighInside, PitchSequence::ChangeEyeLevel),
            (TargetZone::LowOutside, PitchSequence::ChangeSides),
            (TargetZone::Center, PitchSequence::ChangeSpeeds),
        ] {
            let previous = Some(PitchCall {
                pitch_type: PitchType::FourSeamFastball,
                target_zone: zone,
                margin: Margin::Wide,
            });
            let preferences = catcher_preferences(
                CatcherCallingStyle::Adaptive, &batter, CountStatus::C00, previous,
            );
            assert_eq!(preferences.sequence.unwrap().sequence, expected);
        }
    }

    #[test]
    fn generated_catcher_preferences_add_soft_bonuses_without_filtering_calls() {
        let pitcher = pitcher_info();
        let mut batter = batter_info(RL::Right);
        batter.zone_aptitude = ZoneAptitude::InsideDominant;
        let preferences = catcher_preferences(
            CatcherCallingStyle::Cautious, &batter, CountStatus::C00, None,
        );
        let baseline = catcher_pitch_call_proposals(
            &PitchingPreferences::default(), &pitcher, &batter,
            PitchingStrategy::PitchAround, None, usize::MAX,
        );
        let proposals = catcher_pitch_call_proposals(
            &preferences, &pitcher, &batter,
            PitchingStrategy::PitchAround, None, usize::MAX,
        );
        assert_eq!(proposals.len(), baseline.len());
        let zone_preference = preferences.target_zone.unwrap();
        let margin_preference = preferences.margin.unwrap();
        for proposal in proposals {
            let base = baseline.iter().find(|base| {
                base.pitch_call.pitch_type == proposal.pitch_call.pitch_type
                    && base.pitch_call.target_zone == proposal.pitch_call.target_zone
                    && base.pitch_call.margin == proposal.pitch_call.margin
            }).unwrap();
            let zone_bonus = if proposal.pitch_call.target_zone == zone_preference.zone {
                0.8 * zone_preference.strength
            } else { 0.0 };
            let margin_bonus = if proposal.pitch_call.margin == margin_preference.margin {
                0.5 * margin_preference.strength
            } else { 0.0 };
            assert!((proposal.score - base.score - zone_bonus - margin_bonus).abs() < 1e-9);
        }
    }

    #[test]
    fn shortlist_keeps_each_pitch_and_does_not_duplicate_center() {
        let pitcher = pitcher_info();
        let batter = batter_info(RL::Right);
        let calls = shortlist_pitch_calls(
            &pitcher,
            &batter,
            PitchingStrategy::AttackZone,
            None,
            2,
            13,
        );
        assert_eq!(calls.len(), pitcher.pitch_skills.len() * 13);
        for pitch in &pitcher.pitch_skills {
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| {
                        call.pitch_call.pitch_type == pitch.pitch_type
                            && call.pitch_call.target_zone == TargetZone::Center
                    })
                    .count(),
                1
            );
        }

        let limited = shortlist_pitch_calls(
            &pitcher,
            &batter,
            PitchingStrategy::AttackZone,
            None,
            2,
            2,
        );
        assert_eq!(limited.len(), pitcher.pitch_skills.len() * 2);
        assert!(pitcher.pitch_skills.iter().all(|pitch| {
            limited
                .iter()
                .filter(|call| call.pitch_call.pitch_type == pitch.pitch_type)
                .count()
                == 2
        }));
    }

    #[test]
    fn location_risks_reflect_margin_and_batter_aptitude() {
        let pitcher = pitcher_info();
        let mut batter = batter_info(RL::Right);
        batter.zone_aptitude = ZoneAptitude::InsideDominant;
        let calls = shortlist_pitch_calls(
            &pitcher,
            &batter,
            PitchingStrategy::AvoidExtraBases,
            None,
            1,
            13,
        );
        let risk = |zone, margin| {
            calls
                .iter()
                .find(|call| {
                    call.pitch_call.target_zone == zone && call.pitch_call.margin == margin
                })
                .unwrap()
                .risks
        };

        assert!(risk(TargetZone::LowOutside, Margin::Out).walk
            > risk(TargetZone::LowOutside, Margin::Wide).walk);
        assert!(risk(TargetZone::LowInside, Margin::Wide).hard_contact
            > risk(TargetZone::LowOutside, Margin::Wide).hard_contact);
    }

    #[test]
    fn catcher_preferences_can_propose_a_pitch_outside_pitcher_shortlist() {
        let pitcher = pitcher_info();
        let batter = batter_info(RL::Right);
        let pitcher_shortlist = shortlist_pitch_calls(
            &pitcher,
            &batter,
            PitchingStrategy::AttackZone,
            None,
            1,
            1,
        );
        let preferences = PitchingPreferences {
            target_zone: Some(TargetZonePreference {
                zone: TargetZone::LowOutside,
                strength: 1.0,
            }),
            margin: Some(MarginPreference {
                margin: Margin::Edge,
                strength: 1.0,
            }),
            ..PitchingPreferences::default()
        };
        let catcher_calls = catcher_pitch_call_proposals(
            &preferences,
            &pitcher,
            &batter,
            PitchingStrategy::AttackZone,
            None,
            26,
        );

        assert_eq!(catcher_calls.len(), 26); // Two pitches × thirteen distinct locations
        assert!(catcher_calls.iter().any(|call| {
            call.pitch_call.pitch_type != pitcher_shortlist[0].pitch_call.pitch_type
        }));
        assert_eq!(catcher_calls[0].pitch_call.target_zone, TargetZone::LowOutside);
        assert_eq!(catcher_calls[0].pitch_call.margin, Margin::Edge);
    }

    #[test]
    fn catcher_strategy_changes_the_relative_value_of_pitching_outside() {
        let pitcher = pitcher_info();
        let batter = batter_info(RL::Right);
        let preferences = PitchingPreferences::default();
        let attack = catcher_pitch_call_proposals(
            &preferences,
            &pitcher,
            &batter,
            PitchingStrategy::AttackZone,
            None,
            26,
        );
        let pitch_around = catcher_pitch_call_proposals(
            &preferences,
            &pitcher,
            &batter,
            PitchingStrategy::PitchAround,
            None,
            26,
        );
        let pitch_type = pitcher.pitch_skills[0].pitch_type;
        let score = |proposals: &[PitchCallProposal], margin| {
            proposals
                .iter()
                .find(|proposal| {
                    proposal.pitch_call.pitch_type == pitch_type
                        && proposal.pitch_call.target_zone == TargetZone::LowOutside
                        && proposal.pitch_call.margin == margin
                })
                .unwrap()
                .score
        };

        let attack_outside_bonus = score(&attack, Margin::Out) - score(&attack, Margin::Wide);
        let pitch_around_outside_bonus =
            score(&pitch_around, Margin::Out) - score(&pitch_around, Margin::Wide);
        assert!(pitch_around_outside_bonus > attack_outside_bonus);
    }
}
