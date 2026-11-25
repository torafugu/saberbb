//! Observes the live pitch-call shortlists and scores without changing decisions.
use super::{
    CallScoreTerms, CountStatus, Margin, PitchCall, PitchCallDecision, PitchCallProposal,
    PitchEvaluationContext, PitchingPreferences, PitchingStrategy, evaluate_pitch_call_with_terms,
    pitch_call_locations,
};
use crate::domain::shared::{
    ball::BallLocation, game::BattingResult, game_stats::PlayerGameBatting,
};
use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Default, Serialize)]
struct CallCounts {
    pitches: u64,
    out_evaluable: u64,
    out_in_pitcher_shortlist: u64,
    out_in_catcher_shortlist: u64,
    out_in_final_union: u64,
    out_selected: u64,
    out_scores_above_winner_without_shortlist: u64,
}

#[derive(Default, Serialize)]
struct ScoreGaps {
    samples: u64,
    total: f64,
    pitch_usage: f64,
    pitch_type_strategy: f64,
    arsenal_preference: f64,
    location_strategy: f64,
    base_risk: f64,
    location_preferences: f64,
    margin_preference: f64,
}

impl ScoreGaps {
    fn add(&mut self, winner: ScoredCall, outside: ScoredCall) {
        let difference = |term: fn(CallScoreTerms) -> f64| {
            let side = |i: usize| term(winner.terms[i]) - term(outside.terms[i]);
            (side(0) + side(1)) * 0.5
        };
        self.samples += 1;
        self.total += winner.score - outside.score;
        self.pitch_usage += difference(|t| t.pitch_usage);
        self.pitch_type_strategy += difference(|t| t.pitch_type_strategy);
        self.arsenal_preference += difference(|t| t.arsenal_preference);
        self.location_strategy += difference(|t| t.location_strategy);
        self.base_risk += difference(|t| t.base_risk);
        self.location_preferences += difference(|t| t.location_preferences);
        self.margin_preference += difference(|t| t.margin_preference);
    }
}

#[derive(Clone, Copy)]
struct ScoredCall {
    score: f64,
    terms: [CallScoreTerms; 2],
}

struct Observation {
    selected_out: bool,
    in_shortlist: [bool; 2],
    winner: ScoredCall,
    outside: Option<ScoredCall>,
    outside_finalist: Option<ScoredCall>,
}

#[derive(Default, Serialize)]
struct Outcomes {
    recorded_pitches: u64,
    actual_outside: u64,
    outside_swings: u64,
    outside_whiffs: u64,
    batters_faced: u64,
    strikeouts: u64,
    walks: u64,
    home_runs: u64,
}

impl Outcomes {
    fn record(&mut self, outside: bool, swung: bool, result: BattingResult) {
        self.recorded_pitches += 1;
        self.actual_outside += u64::from(outside);
        self.outside_swings += u64::from(outside && swung);
        self.outside_whiffs += u64::from(
            outside
                && swung
                && matches!(
                    result,
                    BattingResult::StrikeSwung | BattingResult::Strikeout
                ),
        );
        self.batters_faced += u64::from(matches!(
            result,
            BattingResult::Strikeout
                | BattingResult::Walk
                | BattingResult::HitByPitch
                | BattingResult::Single
                | BattingResult::Double
                | BattingResult::Triple
                | BattingResult::HomeRun
                | BattingResult::FieldersChoice
                | BattingResult::Out
                | BattingResult::DoublePlay
        ));
        self.strikeouts += u64::from(result == BattingResult::Strikeout);
        self.walks += u64::from(result == BattingResult::Walk);
        self.home_runs += u64::from(result == BattingResult::HomeRun);
    }
}

#[derive(Default, Serialize)]
struct Bucket {
    #[serde(flatten)]
    counts: CallCounts,
    winner_minus_best_out_score_sums: ScoreGaps,
    winner_minus_best_out_finalist_score_sums: ScoreGaps,
    outcomes: Outcomes,
    selected_out_outcomes: Outcomes,
}

impl Bucket {
    fn record_outcome(&mut self, aim: Margin, outside: bool, swung: bool, result: BattingResult) {
        self.outcomes.record(outside, swung, result);
        if aim == Margin::Out {
            self.selected_out_outcomes.record(outside, swung, result);
        }
    }

    fn record(&mut self, pitch: &Observation) {
        let counts = &mut self.counts;
        counts.pitches += 1;
        counts.out_selected += u64::from(pitch.selected_out);
        counts.out_in_pitcher_shortlist += u64::from(pitch.in_shortlist[0]);
        counts.out_in_catcher_shortlist += u64::from(pitch.in_shortlist[1]);
        if let Some(outside) = pitch.outside {
            counts.out_evaluable += 1;
            counts.out_scores_above_winner_without_shortlist +=
                u64::from(outside.score > pitch.winner.score);
            self.winner_minus_best_out_score_sums
                .add(pitch.winner, outside);
        }
        if let Some(finalist) = pitch.outside_finalist {
            counts.out_in_final_union += 1;
            self.winner_minus_best_out_finalist_score_sums
                .add(pitch.winner, finalist);
        }
    }
}

#[derive(Default, Serialize)]
pub struct PitchCallAudit {
    overall: Bucket,
    by_count_and_strategy: BTreeMap<String, BTreeMap<String, Bucket>>,
    by_pitcher: BTreeMap<i64, BTreeMap<String, Bucket>>,
}

thread_local! {
    static ACTIVE: RefCell<Option<PitchCallAudit>> = const { RefCell::new(None) };
}

/// Collect calls made during this synchronous simulation, including nested scope restoration.
pub fn with_pitch_call_audit<T>(run: impl FnOnce() -> T) -> (T, PitchCallAudit) {
    struct Restore(Option<PitchCallAudit>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE.replace(self.0.take());
        }
    }

    let _restore = Restore(ACTIVE.replace(Some(PitchCallAudit::default())));
    let result = run();
    let audit = ACTIVE.take().expect("pitch call audit scope is active");
    (result, audit)
}

pub(crate) fn record(
    pitcher_id: i64,
    count: CountStatus,
    context: &PitchEvaluationContext<'_>,
    preferences: [&PitchingPreferences; 2],
    shortlists: [&[PitchCallProposal]; 2],
    decision: PitchCallDecision,
) {
    ACTIVE.with_borrow_mut(|active| {
        let Some(audit) = active else { return };
        let evaluate = |call| {
            let (pitcher, pitcher_terms) =
                evaluate_pitch_call_with_terms(call, preferences[0], context)?;
            let (catcher, catcher_terms) =
                evaluate_pitch_call_with_terms(call, preferences[1], context)?;
            Some(ScoredCall {
                score: 0.5 * pitcher.score + 0.5 * catcher.score,
                terms: [pitcher_terms, catcher_terms],
            })
        };
        let best_out = |calls: &mut dyn Iterator<Item = PitchCall>| {
            calls
                .filter(|call| call.margin == Margin::Out)
                .filter_map(evaluate)
                .max_by(|a, b| a.score.total_cmp(&b.score))
        };
        let mut full = context
            .pitcher
            .pitch_skills
            .iter()
            .flat_map(|skill| pitch_call_locations(skill.pitch_type));
        let mut finalists = shortlists
            .iter()
            .flat_map(|list| list.iter().map(|p| p.pitch_call));
        let observation = Observation {
            selected_out: decision.pitch_call.margin == Margin::Out,
            in_shortlist: shortlists
                .map(|list| list.iter().any(|p| p.pitch_call.margin == Margin::Out)),
            winner: evaluate(decision.pitch_call)
                .expect("the selected pitch was scored by both sides"),
            outside: best_out(&mut full),
            outside_finalist: best_out(&mut finalists),
        };
        let count = count.as_ref().to_owned();
        audit.overall.record(&observation);
        audit
            .by_count_and_strategy
            .entry(count.clone())
            .or_default()
            .entry(format!("{:?}", context.strategy))
            .or_default()
            .record(&observation);
        audit
            .by_pitcher
            .entry(pitcher_id)
            .or_default()
            .entry(count)
            .or_default()
            .record(&observation);
    });
}

pub(crate) fn record_outcome(
    count: CountStatus,
    strategy: PitchingStrategy,
    aim: Margin,
    location: BallLocation,
    swung: bool,
    batting: &PlayerGameBatting,
) {
    ACTIVE.with_borrow_mut(|active| {
        let Some(audit) = active else { return };
        let count = count.as_ref();
        let outside = location.distance_from_zone_edge() > 0.0;
        let result = batting.result;
        audit.overall.record_outcome(aim, outside, swung, result);
        audit
            .by_count_and_strategy
            .get_mut(count)
            .unwrap()
            .get_mut(&format!("{strategy:?}"))
            .unwrap()
            .record_outcome(aim, outside, swung, result);
        audit
            .by_pitcher
            .get_mut(&batting.pitcher_id)
            .unwrap()
            .get_mut(count)
            .unwrap()
            .record_outcome(aim, outside, swung, result);
    });
}
