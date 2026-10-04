use crate::domain::resolver::batting_resolver::CountStatus;
use crate::domain::shared::ball::{BallLocation, BallZone};
use crate::domain::shared::game_state::InningState;
use crate::domain::shared::player::{
    BatterInfo, CatcherCallingStyle, PitchType, PitcherCharacter, PitcherInfo,
};
use crate::domain::shared::prob::ItemWeighted;
use crate::domain::strategy::common_strategy::{
    DEFAULT_XBH_PROBS, HitAdvanceModel, RunExpectancyTable,
};
use crate::domain::util::sigmoid;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    pub shortlist_score: f64,
}

/// One immutable evaluation snapshot shared by candidate generation and negotiation.
/// Construct a new context when the pitcher, batter, strategy or previous call changes.
pub struct PitchEvaluationContext<'a> {
    pitcher: &'a PitcherInfo,
    batter: &'a BatterInfo,
    strategy: PitchingStrategy,
    previous_call: Option<PitchCall>,
    estimates: HashMap<PitchType, PitchTypeEstimate>,
    pitch_profiles: HashMap<PitchType, (f64, f64)>, // velocity, usage weight
}

impl<'a> PitchEvaluationContext<'a> {
    pub fn new(
        pitcher: &'a PitcherInfo,
        batter: &'a BatterInfo,
        strategy: PitchingStrategy,
        previous_call: Option<PitchCall>,
    ) -> Self {
        Self {
            pitcher,
            batter,
            strategy,
            previous_call,
            estimates: estimate_pitch_types(pitcher, batter),
            pitch_profiles: pitcher
                .pitch_skill_distribution()
                .into_iter()
                .map(|item| (item.name.pitch_type, (item.name.velocity, item.weight)))
                .collect(),
        }
    }

    pub fn pitcher_proposals(
        &self,
        preferences: &PitchingPreferences,
        pitch_type_limit: usize,
        limit_per_pitch: usize,
    ) -> Vec<PitchCallProposal> {
        let types = shortlist_pitch_types(preferences, self, pitch_type_limit);
        shortlist_locations_for_pitch_types(preferences, &types, self, limit_per_pitch)
    }

    pub fn catcher_proposals(
        &self,
        preferences: &PitchingPreferences,
        limit: usize,
    ) -> Vec<PitchCallProposal> {
        let mut proposals = Vec::new();
        for skill in &self.pitcher.pitch_skills {
            for call in pitch_call_locations(skill.pitch_type) {
                if let Some(proposal) = evaluate_pitch_call(call, preferences, self) {
                    proposals.push(proposal);
                }
            }
        }
        proposals.sort_by(|a, b| b.score.total_cmp(&a.score));
        proposals.truncate(limit);
        proposals
    }
}

/// Score a single executable call with the same formula used for both shortlists.
pub fn evaluate_pitch_call(
    call: PitchCall,
    preferences: &PitchingPreferences,
    context: &PitchEvaluationContext<'_>,
) -> Option<PitchCallProposal> {
    let estimate = context.estimates.get(&call.pitch_type)?;
    let &(velocity, usage_weight) = context.pitch_profiles.get(&call.pitch_type)?;
    let risks = estimate_call_risks(call, estimate, context.batter);
    let score = usage_weight.max(1e-9).ln()
        + pitch_type_strategy_score(context.strategy, estimate)
        + arsenal_preference_score(
            preferences,
            call.pitch_type,
            usage_weight,
            context.previous_call,
        )
        + pitch_call_strategy_score(context.strategy, call, estimate, risks)
        + base_call_risk_score(risks)
        + call_preference_score(
            preferences,
            context.pitcher,
            call,
            velocity,
            risks,
            context.previous_call,
        );
    if !score.is_finite()
        || !risks.walk.is_finite()
        || !risks.hard_contact.is_finite()
        || !risks.execution.is_finite()
    {
        return None;
    }
    Some(PitchCallProposal {
        pitch_call: call,
        score,
        risks,
    })
}

fn pitch_call_locations(pitch_type: PitchType) -> impl Iterator<Item = PitchCall> {
    const ZONES: [TargetZone; 5] = [
        TargetZone::Center,
        TargetZone::LowInside,
        TargetZone::LowOutside,
        TargetZone::HighInside,
        TargetZone::HighOutside,
    ];
    const MARGINS: [Margin; 3] = [Margin::Wide, Margin::Edge, Margin::Out];
    ZONES.into_iter().flat_map(move |target_zone| {
        MARGINS.into_iter().filter_map(move |margin| {
            if target_zone == TargetZone::Center && margin != Margin::Wide {
                None
            } else {
                Some(PitchCall {
                    pitch_type,
                    target_zone,
                    margin,
                })
            }
        })
    })
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
    preferences: &PitchingPreferences,
    context: &PitchEvaluationContext<'_>,
    limit: usize,
) -> Vec<PitchTypeProposal> {
    let pitcher = context.pitcher;
    let strategy = context.strategy;
    let previous_call = context.previous_call;
    let mut proposals: Vec<_> = pitcher
        .pitch_skills
        .iter()
        .filter_map(|skill| {
            let pitch_type = skill.pitch_type;
            let &(velocity, usage_weight) = context.pitch_profiles.get(&pitch_type)?;
            let estimate = context.estimates.get(&pitch_type)?;

            // Base usage rate derived from the existing PitchSkill.usage.
            // Convert to log scale to match the score adjustments below.
            let usage_score = usage_weight.max(1e-9).ln();

            let strategy_score = pitch_type_strategy_score(strategy, estimate);

            let score = usage_score
                + strategy_score
                + arsenal_preference_score(preferences, pitch_type, usage_weight, previous_call);
            // Coarse risk and speed-change estimates help prune pitch types.
            // They are not carried into the final score: the call stage evaluates
            // the actual location risk and sequence once.
            let risk_score = preferences.risk.map_or(0.0, |risk| {
                -0.5 * risk.hard_contact_aversion.clamp(0.0, 1.0) * estimate.hard_contact
                    - 0.5 * risk.walk_aversion.clamp(0.0, 1.0) * (1.0 - estimate.command)
            });
            let sequence_score = sequence_preference_score(
                preferences,
                pitcher,
                PitchCall {
                    pitch_type,
                    target_zone: TargetZone::Center,
                    margin: Margin::Wide,
                },
                velocity,
                previous_call,
            );

            Some(PitchTypeProposal {
                pitch_type,
                shortlist_score: score + risk_score + sequence_score,
            })
        })
        .collect();

    // sort_by is stable, preserving the original pitch_skills order for ties.
    proposals.sort_by(|a, b| b.shortlist_score.total_cmp(&a.shortlist_score));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PitchCallDecisionReason {
    Agreement,
    ReevaluatedAgreement,
    PitcherChoice,
    CatcherChoice,
}

/// Scores are relative utilities, not probabilities. Both scores are populated
/// after successful negotiation; Option fields are retained for existing consumers.
#[derive(Debug, Clone, Copy)]
pub struct PitchCallDecision {
    pub pitch_call: PitchCall,
    pub score: f64,
    pub pitcher_score: Option<f64>,
    pub catcher_score: Option<f64>,
    pub reason: PitchCallDecisionReason,
}

/// Choose a final call after evaluating the candidate union with equal influence.
pub fn select_pitch_call(
    pitcher_proposals: &[PitchCallProposal],
    catcher_proposals: &[PitchCallProposal],
    pitcher_preferences: &PitchingPreferences,
    catcher_preferences: &PitchingPreferences,
    context: &PitchEvaluationContext<'_>,
) -> Option<PitchCall> {
    reconcile_pitch_call_proposals(
        pitcher_proposals,
        catcher_proposals,
        pitcher_preferences,
        catcher_preferences,
        context,
        0.5,
    )
    .map(|decision| decision.pitch_call)
}

/// Evaluate only the distinct calls proposed by either side. Original shortlist
/// scores are deliberately ignored: both sides use the current evaluation snapshot.
/// Unavailable pitches and non-finite evaluations are excluded. No valid calls: None.
/// Ties prefer the pitcher score, then pitcher-first union order.
/// Weights are clamped to 0..=1; non-finite weights use 0.5.
pub fn reconcile_pitch_call_proposals(
    pitcher_proposals: &[PitchCallProposal],
    catcher_proposals: &[PitchCallProposal],
    pitcher_preferences: &PitchingPreferences,
    catcher_preferences: &PitchingPreferences,
    context: &PitchEvaluationContext<'_>,
    catcher_weight: f64,
) -> Option<PitchCallDecision> {
    let pitcher_calls: HashSet<_> = pitcher_proposals.iter().map(|p| p.pitch_call).collect();
    let catcher_calls: HashSet<_> = catcher_proposals.iter().map(|p| p.pitch_call).collect();
    let mut seen = HashSet::new();
    let mut pitcher_evaluations = Vec::new();
    let mut catcher_evaluations = Vec::new();
    for proposal in pitcher_proposals.iter().chain(catcher_proposals) {
        if !seen.insert(proposal.pitch_call) {
            continue;
        }
        // A single immutable context supplies the same capability/risk estimates.
        let (Some(pitcher), Some(catcher)) = (
            evaluate_pitch_call(proposal.pitch_call, pitcher_preferences, context),
            evaluate_pitch_call(proposal.pitch_call, catcher_preferences, context),
        ) else {
            continue;
        };
        pitcher_evaluations.push(pitcher);
        catcher_evaluations.push(catcher);
    }
    let mut decision =
        combine_evaluated_pitch_calls(&pitcher_evaluations, &catcher_evaluations, catcher_weight)?;
    if decision.reason == PitchCallDecisionReason::Agreement
        && !(pitcher_calls.contains(&decision.pitch_call)
            && catcher_calls.contains(&decision.pitch_call))
    {
        decision.reason = PitchCallDecisionReason::ReevaluatedAgreement;
    }
    Some(decision)
}

fn combine_evaluated_pitch_calls(
    pitcher_proposals: &[PitchCallProposal],
    catcher_proposals: &[PitchCallProposal],
    catcher_weight: f64,
) -> Option<PitchCallDecision> {
    let weight = if catcher_weight.is_finite() {
        catcher_weight.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let reason = if weight == 0.0 {
        PitchCallDecisionReason::PitcherChoice
    } else if weight == 1.0 {
        PitchCallDecisionReason::CatcherChoice
    } else {
        PitchCallDecisionReason::Agreement
    };
    let mut best: Option<PitchCallDecision> = None;
    // The two evaluations were produced together in the same union order.
    for (pitcher, catcher) in pitcher_proposals.iter().zip(catcher_proposals) {
        let score = (1.0 - weight) * pitcher.score + weight * catcher.score;
        if best.is_none_or(|current| {
            score > current.score
                || (score == current.score && Some(pitcher.score) > current.pitcher_score)
        }) {
            best = Some(PitchCallDecision {
                pitch_call: pitcher.pitch_call,
                score,
                pitcher_score: Some(pitcher.score),
                catcher_score: Some(catcher.score),
                reason,
            });
        }
    }
    best
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
        PitchingStrategy::HuntStrikeout => 0.3 * estimate.whiff * edge.min(1.0) - 0.3 * risks.walk,
        PitchingStrategy::InduceGroundBall => {
            0.4 * estimate.ground_ball * f64::from(aim.y < 0.0) - 0.3 * risks.walk
        }
        PitchingStrategy::AvoidExtraBases => -0.8 * risks.hard_contact,
        PitchingStrategy::PitchAround => -0.8 * risks.hard_contact + 0.3 * outside,
    }
}

/// Concretize previously generated preferences without reading pitcher character.
/// Keep a separate location shortlist for each pitch type.
pub fn pitcher_pitch_call_proposals(
    preferences: &PitchingPreferences,
    pitcher: &PitcherInfo,
    batter: &BatterInfo,
    strategy: PitchingStrategy,
    previous_call: Option<PitchCall>,
    pitch_type_limit: usize,
    limit_per_pitch: usize,
) -> Vec<PitchCallProposal> {
    PitchEvaluationContext::new(pitcher, batter, strategy, previous_call).pitcher_proposals(
        preferences,
        pitch_type_limit,
        limit_per_pitch,
    )
}

fn shortlist_locations_for_pitch_types(
    preferences: &PitchingPreferences,
    pitch_types: &[PitchTypeProposal],
    context: &PitchEvaluationContext<'_>,
    limit_per_pitch: usize,
) -> Vec<PitchCallProposal> {
    let mut shortlisted = Vec::new();
    for pitch in pitch_types {
        let mut proposals: Vec<_> = pitch_call_locations(pitch.pitch_type)
            .filter_map(|call| evaluate_pitch_call(call, preferences, context))
            .collect();
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
    _inning: u8,
    _score_diff: i16, // If the score is a plus from the defensive team's perspective, it is a lead.
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
    pub risk: Option<RiskPreference>,
}

/// Subjective aversion weights, not estimated event probabilities.
#[derive(Debug, Clone, Copy)]
pub struct RiskPreference {
    pub walk_aversion: f64,
    pub hard_contact_aversion: f64,
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

/// Convert character into wishes; actual skill, strategy and risk estimates
/// belong to pitcher_pitch_call_proposals. Tendencies remain an internal input.
pub fn pitcher_preferences(
    character: PitcherCharacter,
    batter: &BatterInfo,
    count: CountStatus,
    previous_call: Option<PitchCall>,
) -> PitchingPreferences {
    let tendencies = character.tendencies();
    let avoids_walks = tendencies.walk_aversion > tendencies.hard_contact_aversion;
    let prefers_mix = tendencies.pitch_variety >= 0.5;
    let mut preferences = PitchingPreferences {
        target_zone: batter_weak_zone_preference(batter),
        margin: Some(MarginPreference {
            margin: if avoids_walks {
                Margin::Wide
            } else {
                Margin::Edge
            },
            strength: tendencies
                .walk_aversion
                .max(tendencies.hard_contact_aversion),
        }),
        arsenal: Some(ArsenalPreference {
            arsenal: if prefers_mix {
                PitchArsenal::Mix
            } else {
                PitchArsenal::BestPitch
            },
            strength: if prefers_mix {
                tendencies.pitch_variety
            } else {
                1.0 - tendencies.pitch_variety
            },
        }),
        sequence: previous_call.map(|last| SequencePreference {
            sequence: match character {
                PitcherCharacter::Aggressive => PitchSequence::ChangeEyeLevel,
                PitcherCharacter::Cautious => PitchSequence::ChangeSides,
                PitcherCharacter::Flexible => adaptive_sequence(last),
                PitcherCharacter::Balanced => PitchSequence::ChangeSpeeds,
            },
            strength: tendencies.pitch_variety,
        }),
        risk: Some(RiskPreference {
            walk_aversion: tendencies.walk_aversion,
            hard_contact_aversion: tendencies.hard_contact_aversion,
        }),
    };
    apply_count_preferences(&mut preferences, count, previous_call);
    preferences
}

fn adaptive_sequence(last: PitchCall) -> PitchSequence {
    let aim = last.aim_location();
    if aim.y > 0.0 {
        PitchSequence::ChangeEyeLevel
    } else if aim.x != 0.0 {
        PitchSequence::ChangeSides
    } else {
        PitchSequence::ChangeSpeeds
    }
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
        target_zone: batter_weak_zone_preference(batter),
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
                adaptive_sequence(last)
            } else {
                sequence
            };
            SequencePreference {
                sequence,
                strength: sequence_strength,
            }
        }),
        risk: None,
    };

    apply_count_preferences(&mut preferences, count, previous_call);
    preferences
}

fn apply_count_preferences(
    preferences: &mut PitchingPreferences,
    count: CountStatus,
    previous_call: Option<PitchCall>,
) {
    // Three balls take precedence over two strikes, including a full count.
    // These are count-based wishes; PitchAround can still favor Out in proposals.
    if matches!(
        count,
        CountStatus::C30 | CountStatus::C31 | CountStatus::C32
    ) {
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
}

fn batter_weak_zone_preference(batter: &BatterInfo) -> Option<TargetZonePreference> {
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

fn arsenal_preference_score(
    preferences: &PitchingPreferences,
    pitch_type: PitchType,
    usage_weight: f64,
    previous_call: Option<PitchCall>,
) -> f64 {
    preferences.arsenal.map_or(0.0, |preferred| {
        let bonus = match preferred.arsenal {
            // Usage is the current proxy for the pitcher's trusted pitch.
            PitchArsenal::BestPitch => usage_weight,
            PitchArsenal::Mix => {
                f64::from(previous_call.is_some_and(|call| call.pitch_type != pitch_type))
            }
        };
        0.3 * preferred.strength.clamp(0.0, 1.0) * bonus
    })
}

fn sequence_preference_score(
    preferences: &PitchingPreferences,
    pitcher: &PitcherInfo,
    pitch_call: PitchCall,
    velocity: f64,
    previous_call: Option<PitchCall>,
) -> f64 {
    let (Some(preferred), Some(last_call)) = (preferences.sequence, previous_call) else {
        return 0.0;
    };
    let aim = pitch_call.aim_location();
    let last_aim = last_call.aim_location();
    let bonus = match preferred.sequence {
        PitchSequence::ChangeSpeeds => pitcher
            .pitch_skills
            .iter()
            .find(|skill| skill.pitch_type == last_call.pitch_type)
            .map_or(0.0, |last_skill| {
                let last_velocity = last_skill.velocity;
                (velocity - last_velocity).abs() / velocity.abs().max(last_velocity.abs()).max(1e-9)
            }),
        PitchSequence::ChangeEyeLevel => f64::from(aim.y * last_aim.y < 0.0),
        PitchSequence::ChangeSides => f64::from(aim.x * last_aim.x < 0.0),
    };
    0.3 * preferred.strength.clamp(0.0, 1.0) * bonus
}

fn call_preference_score(
    preferences: &PitchingPreferences,
    pitcher: &PitcherInfo,
    pitch_call: PitchCall,
    velocity: f64,
    risks: PitchRisks,
    previous_call: Option<PitchCall>,
) -> f64 {
    let mut score = 0.0;
    if let Some(preferred) = preferences.target_zone {
        if pitch_call.target_zone == preferred.zone {
            score += 0.8 * preferred.strength.clamp(0.0, 1.0);
        }
    }
    if let Some(preferred) = preferences.margin {
        if pitch_call.margin == preferred.margin {
            score += 0.5 * preferred.strength.clamp(0.0, 1.0);
        }
    }
    if let Some(preferred) = preferences.risk {
        score -= 0.5 * preferred.walk_aversion.clamp(0.0, 1.0) * risks.walk
            + 0.5 * preferred.hard_contact_aversion.clamp(0.0, 1.0) * risks.hard_contact;
    }
    score + sequence_preference_score(preferences, pitcher, pitch_call, velocity, previous_call)
}

fn base_call_risk_score(risks: PitchRisks) -> f64 {
    -0.15 * risks.walk - 0.25 * risks.hard_contact - 0.15 * risks.execution
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
    PitchEvaluationContext::new(pitcher, batter, strategy, previous_call)
        .catcher_proposals(preferences, limit)
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
mod pitch_call_reconciliation_tests {
    use super::*;
    use crate::domain::shared::player::RL;
    use crate::domain::test_support::{batter_info, pitcher_info};

    fn call(zone: TargetZone) -> PitchCall {
        PitchCall {
            pitch_type: PitchType::FourSeamFastball,
            target_zone: zone,
            margin: Margin::Edge,
        }
    }

    fn proposal(call: PitchCall, score: f64) -> PitchCallProposal {
        PitchCallProposal {
            pitch_call: call,
            score,
            risks: PitchRisks {
                walk: 0.0,
                hard_contact: 0.0,
                execution: 0.0,
            },
        }
    }

    fn prefers(zone: TargetZone, strength: f64) -> PitchingPreferences {
        PitchingPreferences {
            target_zone: Some(TargetZonePreference { zone, strength }),
            ..PitchingPreferences::default()
        }
    }

    #[test]
    fn catcher_only_candidate_can_win_even_when_a_common_candidate_exists() {
        let pitcher = pitcher_info();
        let batter = batter_info(RL::Right);
        let context =
            PitchEvaluationContext::new(&pitcher, &batter, PitchingStrategy::AttackZone, None);
        let inside = call(TargetZone::LowInside);
        let outside = call(TargetZone::LowOutside);
        let pitcher_preferences = prefers(TargetZone::LowInside, 0.2);
        let catcher_preferences = prefers(TargetZone::LowOutside, 1.0);
        let p = [proposal(inside, 1000.0)];
        let c = [proposal(inside, 1000.0), proposal(outside, -1000.0)];
        let decision = reconcile_pitch_call_proposals(
            &p,
            &c,
            &pitcher_preferences,
            &catcher_preferences,
            &context,
            0.5,
        )
        .unwrap();
        assert_eq!(decision.pitch_call, outside);
        assert_eq!(
            decision.reason,
            PitchCallDecisionReason::ReevaluatedAgreement
        );
        let ps = evaluate_pitch_call(outside, &pitcher_preferences, &context)
            .unwrap()
            .score;
        let cs = evaluate_pitch_call(outside, &catcher_preferences, &context)
            .unwrap()
            .score;
        assert_eq!(decision.pitcher_score, Some(ps));
        assert_eq!(decision.catcher_score, Some(cs));
        assert!((decision.score - (ps + cs) / 2.0).abs() < 1e-9);
        assert_eq!(
            select_pitch_call(&p, &c, &pitcher_preferences, &catcher_preferences, &context),
            Some(outside)
        );
    }

    #[test]
    fn disjoint_and_single_side_lists_are_evaluated_by_both_sides() {
        let pitcher = pitcher_info();
        let batter = batter_info(RL::Right);
        let context =
            PitchEvaluationContext::new(&pitcher, &batter, PitchingStrategy::AttackZone, None);
        let prefs = prefers(TargetZone::LowOutside, 1.0);
        let p = [proposal(call(TargetZone::LowInside), f64::NAN)];
        let c = [proposal(call(TargetZone::LowOutside), f64::NEG_INFINITY)];
        for (p, c) in [(&p[..], &c[..]), (&[][..], &c[..]), (&c[..], &[][..])] {
            let decision =
                reconcile_pitch_call_proposals(p, c, &prefs, &prefs, &context, 0.5).unwrap();
            assert_eq!(decision.pitch_call, call(TargetZone::LowOutside));
            assert!(decision.pitcher_score.is_some() && decision.catcher_score.is_some());
            assert_eq!(
                decision.reason,
                PitchCallDecisionReason::ReevaluatedAgreement
            );
        }
        assert!(select_pitch_call(&[], &[], &prefs, &prefs, &context).is_none());
    }

    #[test]
    fn unavailable_pitches_are_excluded_even_when_proposed_with_a_high_score() {
        let pitcher = pitcher_info();
        let batter = batter_info(RL::Right);
        let context =
            PitchEvaluationContext::new(&pitcher, &batter, PitchingStrategy::AttackZone, None);
        let prefs = PitchingPreferences::default();
        let unsupported = PitchCall {
            pitch_type: PitchType::Changeup,
            ..call(TargetZone::LowInside)
        };
        assert!(evaluate_pitch_call(unsupported, &prefs, &context).is_none());
        let invalid = [proposal(unsupported, 1e6)];
        assert!(select_pitch_call(&invalid, &[], &prefs, &prefs, &context).is_none());
        let valid = [proposal(call(TargetZone::HighOutside), -1e6)];
        assert_eq!(
            select_pitch_call(&invalid, &valid, &prefs, &prefs, &context),
            Some(valid[0].pitch_call)
        );
        let invalid_preferences = prefers(TargetZone::HighOutside, f64::NAN);
        assert!(select_pitch_call(&valid, &[], &invalid_preferences, &prefs, &context,).is_none());
    }

    #[test]
    fn duplicates_old_scores_and_weights_do_not_break_deterministic_selection() {
        let pitcher = pitcher_info();
        let batter = batter_info(RL::Right);
        let context =
            PitchEvaluationContext::new(&pitcher, &batter, PitchingStrategy::AttackZone, None);
        let p_prefs = prefers(TargetZone::LowInside, 1.0);
        let c_prefs = prefers(TargetZone::LowOutside, 1.0);
        let p = [
            proposal(call(TargetZone::LowInside), -1.0),
            proposal(call(TargetZone::LowInside), 1e6),
        ];
        let c = [proposal(call(TargetZone::LowOutside), 1e6)];
        for (weight, expected) in [
            (-1.0, p[0].pitch_call),
            (0.0, p[0].pitch_call),
            (1.0, c[0].pitch_call),
            (2.0, c[0].pitch_call),
        ] {
            let d = reconcile_pitch_call_proposals(&p, &c, &p_prefs, &c_prefs, &context, weight)
                .unwrap();
            assert_eq!(d.pitch_call, expected);
        }
        let default =
            reconcile_pitch_call_proposals(&p, &c, &p_prefs, &c_prefs, &context, 0.5).unwrap();
        assert_eq!(
            reconcile_pitch_call_proposals(&p, &c, &p_prefs, &c_prefs, &context, f64::NAN)
                .unwrap()
                .pitch_call,
            default.pitch_call
        );
        let neutral = PitchingPreferences::default();
        // Balanced aptitude makes these two corner scores equal.
        let tie = select_pitch_call(&p, &c, &neutral, &neutral, &context).unwrap();
        assert_eq!(tie, p[0].pitch_call);
    }

    #[test]
    fn candidate_generation_and_negotiation_share_the_single_call_evaluator() {
        let pitcher = pitcher_info();
        let batter = batter_info(RL::Right);
        let previous = Some(call(TargetZone::HighInside));
        for strategy in STRATEGY_PRIORITY {
            let context = PitchEvaluationContext::new(&pitcher, &batter, strategy, previous);
            let pp = pitcher_preferences(
                pitcher.pitcher_character,
                &batter,
                CountStatus::C12,
                previous,
            );
            let cp = catcher_preferences(
                CatcherCallingStyle::Adaptive,
                &batter,
                CountStatus::C12,
                previous,
            );
            let p = context.pitcher_proposals(&pp, 2, 2);
            let c = context.catcher_proposals(&cp, 4);
            for (list, prefs) in [(&p, &pp), (&c, &cp)] {
                for proposal in list {
                    let evaluated =
                        evaluate_pitch_call(proposal.pitch_call, prefs, &context).unwrap();
                    assert_eq!(proposal.score, evaluated.score);
                    assert_eq!(proposal.risks.walk, evaluated.risks.walk);
                }
            }
            let decision = reconcile_pitch_call_proposals(&p, &c, &pp, &cp, &context, 0.5).unwrap();
            assert!(
                p.iter()
                    .chain(&c)
                    .any(|p| p.pitch_call == decision.pitch_call)
            );
            for proposal in p.iter().chain(&c) {
                let a = evaluate_pitch_call(proposal.pitch_call, &pp, &context).unwrap();
                let b = evaluate_pitch_call(proposal.pitch_call, &cp, &context).unwrap();
                assert!(decision.score >= (a.score + b.score) / 2.0 - 1e-9);
            }
        }
    }
}

#[cfg(test)]
mod pitch_call_shortlist_tests {
    use super::*;
    use crate::domain::shared::player::{RL, ZoneAptitude};
    use crate::domain::test_support::{batter_info, pitcher_info};

    #[test]
    fn pitcher_character_generates_preferences_and_count_overrides() {
        let batter = batter_info(RL::Right);
        let previous = Some(PitchCall {
            pitch_type: PitchType::FourSeamFastball,
            target_zone: TargetZone::HighInside,
            margin: Margin::Edge,
        });
        let aggressive = pitcher_preferences(
            PitcherCharacter::Aggressive,
            &batter,
            CountStatus::C00,
            previous,
        );
        let cautious = pitcher_preferences(
            PitcherCharacter::Cautious,
            &batter,
            CountStatus::C00,
            previous,
        );
        let flexible = pitcher_preferences(
            PitcherCharacter::Flexible,
            &batter,
            CountStatus::C00,
            previous,
        );
        assert_eq!(aggressive.margin.unwrap().margin, Margin::Wide);
        assert_eq!(cautious.margin.unwrap().margin, Margin::Edge);
        assert!(aggressive.risk.unwrap().walk_aversion > cautious.risk.unwrap().walk_aversion);
        assert!(
            cautious.risk.unwrap().hard_contact_aversion
                > aggressive.risk.unwrap().hard_contact_aversion
        );
        assert_eq!(flexible.arsenal.unwrap().arsenal, PitchArsenal::Mix);
        assert_eq!(
            flexible.sequence.unwrap().sequence,
            PitchSequence::ChangeEyeLevel
        );

        for character in [
            PitcherCharacter::Aggressive,
            PitcherCharacter::Cautious,
            PitcherCharacter::Flexible,
            PitcherCharacter::Balanced,
        ] {
            let full_count = pitcher_preferences(character, &batter, CountStatus::C32, previous);
            assert_eq!(full_count.margin.unwrap().margin, Margin::Wide);
            assert_eq!(full_count.arsenal.unwrap().arsenal, PitchArsenal::BestPitch);
            let two_strikes = pitcher_preferences(character, &batter, CountStatus::C02, previous);
            assert_eq!(two_strikes.margin.unwrap().margin, Margin::Edge);
            let initial = pitcher_preferences(character, &batter, CountStatus::C00, None);
            assert!(initial.sequence.is_none());
            assert!(
                !initial
                    .arsenal
                    .is_some_and(|p| p.arsenal == PitchArsenal::Mix)
            );
        }
    }

    #[test]
    fn proposals_use_explicit_preferences_instead_of_reading_character_again() {
        let mut pitcher = pitcher_info();
        let batter = batter_info(RL::Right);
        let preferences =
            pitcher_preferences(PitcherCharacter::Cautious, &batter, CountStatus::C00, None);
        pitcher.pitcher_character = PitcherCharacter::Aggressive;
        let before = pitcher_pitch_call_proposals(
            &preferences,
            &pitcher,
            &batter,
            PitchingStrategy::AttackZone,
            None,
            2,
            13,
        );
        pitcher.pitcher_character = PitcherCharacter::Flexible;
        let after = pitcher_pitch_call_proposals(
            &preferences,
            &pitcher,
            &batter,
            PitchingStrategy::AttackZone,
            None,
            2,
            13,
        );
        assert_eq!(before.len(), after.len());
        for (a, b) in before.iter().zip(after.iter()) {
            assert_eq!(a.pitch_call.pitch_type, b.pitch_call.pitch_type);
            assert_eq!(a.pitch_call.target_zone, b.pitch_call.target_zone);
            assert_eq!(a.pitch_call.margin, b.pitch_call.margin);
            assert_eq!(a.score, b.score);
        }
    }

    #[test]
    fn pitcher_and_catcher_score_the_same_preferences_consistently() {
        let pitcher = pitcher_info();
        let batter = batter_info(RL::Right);
        let previous = Some(PitchCall {
            pitch_type: PitchType::FourSeamFastball,
            target_zone: TargetZone::HighInside,
            margin: Margin::Edge,
        });
        for sequence in [
            PitchSequence::ChangeSpeeds,
            PitchSequence::ChangeEyeLevel,
            PitchSequence::ChangeSides,
        ] {
            let mut preferences = pitcher_preferences(
                PitcherCharacter::Flexible,
                &batter,
                CountStatus::C12,
                previous,
            );
            preferences.sequence = Some(SequencePreference {
                sequence,
                strength: 0.8,
            });
            for strategy in STRATEGY_PRIORITY {
                let pitcher_calls = pitcher_pitch_call_proposals(
                    &preferences,
                    &pitcher,
                    &batter,
                    strategy,
                    previous,
                    usize::MAX,
                    13,
                );
                let catcher_calls = catcher_pitch_call_proposals(
                    &preferences,
                    &pitcher,
                    &batter,
                    strategy,
                    previous,
                    usize::MAX,
                );
                assert_eq!(pitcher_calls.len(), catcher_calls.len());
                for call in pitcher_calls {
                    let catcher_call = catcher_calls
                        .iter()
                        .find(|candidate| {
                            candidate.pitch_call.pitch_type == call.pitch_call.pitch_type
                                && candidate.pitch_call.target_zone == call.pitch_call.target_zone
                                && candidate.pitch_call.margin == call.pitch_call.margin
                        })
                        .unwrap();
                    // Shortlist-only estimates must not be added to final scores.
                    assert!((call.score - catcher_call.score).abs() < 1e-9);
                }
            }
        }
    }

    #[test]
    fn pitcher_shortlist_applies_mix_and_speed_wishes_before_pruning() {
        let mut pitcher = pitcher_info();
        for skill in &mut pitcher.pitch_skills {
            skill.usage = 0.5;
        }
        let batter = batter_info(RL::Right);
        let previous = Some(PitchCall {
            pitch_type: PitchType::FourSeamFastball,
            target_zone: TargetZone::Center,
            margin: Margin::Wide,
        });
        for preferences in [
            PitchingPreferences {
                arsenal: Some(ArsenalPreference {
                    arsenal: PitchArsenal::Mix,
                    strength: 1.0,
                }),
                ..PitchingPreferences::default()
            },
            PitchingPreferences {
                sequence: Some(SequencePreference {
                    sequence: PitchSequence::ChangeSpeeds,
                    strength: 1.0,
                }),
                ..PitchingPreferences::default()
            },
        ] {
            let calls = pitcher_pitch_call_proposals(
                &preferences,
                &pitcher,
                &batter,
                PitchingStrategy::AttackZone,
                previous,
                1,
                2,
            );
            assert_eq!(calls.len(), 2);
            assert!(
                calls
                    .iter()
                    .all(|call| call.pitch_call.pitch_type == PitchType::Slider)
            );
        }
        let preferences = PitchingPreferences::default();
        assert!(
            pitcher_pitch_call_proposals(
                &preferences,
                &pitcher,
                &batter,
                PitchingStrategy::AttackZone,
                previous,
                0,
                2
            )
            .is_empty()
        );
        assert!(
            pitcher_pitch_call_proposals(
                &preferences,
                &pitcher,
                &batter,
                PitchingStrategy::AttackZone,
                previous,
                2,
                0
            )
            .is_empty()
        );
        pitcher.pitch_skills.clear();
        assert!(
            pitcher_pitch_call_proposals(
                &preferences,
                &pitcher,
                &batter,
                PitchingStrategy::AttackZone,
                previous,
                2,
                2
            )
            .is_empty()
        );
    }

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
                assert_eq!(
                    preferences.arsenal.unwrap().arsenal,
                    PitchArsenal::BestPitch
                );
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
        assert!(
            catcher_preferences(
                CatcherCallingStyle::Balanced,
                &batter,
                CountStatus::C00,
                None,
            )
            .target_zone
            .is_none()
        );

        batter.zone_aptitude = ZoneAptitude::InsideDominant;
        let preference = batter_weak_zone_preference(&batter).unwrap();
        assert!(matches!(
            preference.zone,
            TargetZone::LowOutside | TargetZone::HighOutside
        ));
        assert!(preference.strength > 0.0 && preference.strength <= 0.7);

        batter.zone_aptitude = ZoneAptitude::LowBaller;
        let preference = batter_weak_zone_preference(&batter).unwrap();
        assert!(matches!(
            preference.zone,
            TargetZone::HighInside | TargetZone::HighOutside
        ));
    }

    #[test]
    fn catcher_style_and_previous_call_control_sequence_wishes() {
        let batter = batter_info(RL::Right);
        let initial = catcher_preferences(
            CatcherCallingStyle::Adaptive,
            &batter,
            CountStatus::C00,
            None,
        );
        assert!(initial.sequence.is_none());
        assert!(initial.arsenal.is_none());
        let aggressive = catcher_preferences(
            CatcherCallingStyle::Aggressive,
            &batter,
            CountStatus::C00,
            None,
        );
        let cautious = catcher_preferences(
            CatcherCallingStyle::Cautious,
            &batter,
            CountStatus::C00,
            None,
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
                CatcherCallingStyle::Adaptive,
                &batter,
                CountStatus::C00,
                previous,
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
            CatcherCallingStyle::Cautious,
            &batter,
            CountStatus::C00,
            None,
        );
        let baseline = catcher_pitch_call_proposals(
            &PitchingPreferences::default(),
            &pitcher,
            &batter,
            PitchingStrategy::PitchAround,
            None,
            usize::MAX,
        );
        let proposals = catcher_pitch_call_proposals(
            &preferences,
            &pitcher,
            &batter,
            PitchingStrategy::PitchAround,
            None,
            usize::MAX,
        );
        assert_eq!(proposals.len(), baseline.len());
        let zone_preference = preferences.target_zone.unwrap();
        let margin_preference = preferences.margin.unwrap();
        for proposal in proposals {
            let base = baseline
                .iter()
                .find(|base| {
                    base.pitch_call.pitch_type == proposal.pitch_call.pitch_type
                        && base.pitch_call.target_zone == proposal.pitch_call.target_zone
                        && base.pitch_call.margin == proposal.pitch_call.margin
                })
                .unwrap();
            let zone_bonus = if proposal.pitch_call.target_zone == zone_preference.zone {
                0.8 * zone_preference.strength
            } else {
                0.0
            };
            let margin_bonus = if proposal.pitch_call.margin == margin_preference.margin {
                0.5 * margin_preference.strength
            } else {
                0.0
            };
            assert!((proposal.score - base.score - zone_bonus - margin_bonus).abs() < 1e-9);
        }
    }

    #[test]
    fn shortlist_keeps_each_pitch_and_does_not_duplicate_center() {
        let pitcher = pitcher_info();
        let batter = batter_info(RL::Right);
        let calls = pitcher_pitch_call_proposals(
            &PitchingPreferences::default(),
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

        let limited = pitcher_pitch_call_proposals(
            &PitchingPreferences::default(),
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
        let calls = pitcher_pitch_call_proposals(
            &PitchingPreferences::default(),
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

        assert!(
            risk(TargetZone::LowOutside, Margin::Out).walk
                > risk(TargetZone::LowOutside, Margin::Wide).walk
        );
        assert!(
            risk(TargetZone::LowInside, Margin::Wide).hard_contact
                > risk(TargetZone::LowOutside, Margin::Wide).hard_contact
        );
    }

    #[test]
    fn catcher_preferences_can_propose_a_pitch_outside_pitcher_shortlist() {
        let pitcher = pitcher_info();
        let batter = batter_info(RL::Right);
        let pitcher_shortlist = pitcher_pitch_call_proposals(
            &PitchingPreferences::default(),
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
        assert_eq!(
            catcher_calls[0].pitch_call.target_zone,
            TargetZone::LowOutside
        );
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
