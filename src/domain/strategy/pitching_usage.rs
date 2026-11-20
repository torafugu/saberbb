//! Select pitchers using pregame workload, in-game pitches and an ability evaluation.
use super::pitcher_evaluation::{PitcherAttributeRating, PitcherAttributeWeights, rotation_score};
use crate::domain::shared::{
    pitcher_fatigue::PitcherFatigue,
    player::{Player, Position},
    stats::SeasonPitchingStats,
    team::Team,
};
use std::collections::BTreeMap;

const MAX_ROTATION_SIZE: usize = 5;

struct Candidate<'a> {
    player: &'a Player,
    fatigue: PitcherFatigue,
    ability_score: f64,
    rotation_score: f64,
}

impl Candidate<'_> {
    fn fatigue(&self, pitches: u32) -> f64 {
        PitcherFatigue {
            pitches,
            ..self.fatigue
        }
        .leaving_ratio()
    }
}

/// Roster and workload inputs for a single team's pitcher selection policy.
pub(crate) struct PitchingStaff<'a> {
    candidates: Vec<Candidate<'a>>,
    rotation_size: usize,
}

impl<'a> PitchingStaff<'a> {
    pub(crate) fn new(
        team: &'a Team,
        loads: &BTreeMap<i64, f64>,
        base_four_seam_speed: f64,
        weights: PitcherAttributeWeights,
        season: &SeasonPitchingStats,
    ) -> Self {
        let starter_workload = PitcherFatigue::reference_pitches(0.0);
        let mut candidates: Vec<_> = team
            .players
            .iter()
            .filter(|player| player.defense_skills.position == Position::P)
            .filter_map(|player| {
                let pitcher = player.defense_skills.pitcher.as_ref()?;
                let rating = PitcherAttributeRating::measure(pitcher, base_four_seam_speed);
                let ability_score =
                    rating.map_or(f64::NEG_INFINITY, |rating| weights.score(rating));
                let reference_pitches = PitcherFatigue::reference_pitches(pitcher.stamina);
                // Preserve ability ordering above the opening workload target
                // and stamina ordering below it. Season results can cross it.
                let prior = if reference_pitches >= starter_workload {
                    0.5 + 0.5 * ability_score.max(0.0)
                } else {
                    0.5 * reference_pitches / starter_workload
                };
                Some(Candidate {
                    player,
                    ability_score,
                    rotation_score: rotation_score(prior, player.info.id, season),
                    fatigue: PitcherFatigue {
                        entering_load: loads.get(&player.info.id).copied().unwrap_or_default(),
                        reference_pitches,
                        pitches: 0,
                    },
                })
            })
            .collect();

        candidates.sort_by(|a, b| {
            b.rotation_score
                .total_cmp(&a.rotation_score)
                .then_with(|| {
                    b.fatigue
                        .reference_pitches
                        .total_cmp(&a.fatigue.reference_pitches)
                })
                .then_with(|| a.player.info.id.cmp(&b.player.info.id))
        });
        let rotation_size = candidates
            .len()
            .saturating_sub(1)
            .clamp(1, MAX_ROTATION_SIZE)
            .min(candidates.len());
        Self {
            candidates,
            rotation_size,
        }
    }

    pub(crate) fn starter(&self) -> Option<i64> {
        Self::freshest(self.candidates[..self.rotation_size].iter())
            .map(|candidate| candidate.player.info.id)
    }

    pub(crate) fn reliever(
        &self,
        active_id: i64,
        game_pitches: &BTreeMap<i64, u32>,
    ) -> Option<&'a Player> {
        let pitches = *game_pitches.get(&active_id)?;
        let active = self
            .candidates
            .iter()
            .find(|c| c.player.info.id == active_id)?;
        if active.fatigue(pitches) < 1.0 {
            return None;
        }

        self.candidates[self.rotation_size..]
            .iter()
            .filter(|c| !game_pitches.contains_key(&c.player.info.id) && c.fatigue(0) < 1.0)
            .min_by(|a, b| {
                b.ability_score
                    .total_cmp(&a.ability_score)
                    .then_with(|| compare_fatigue(a, b))
            })
            .map(|candidate| candidate.player)
    }

    fn freshest<'b>(
        candidates: impl Iterator<Item = &'b Candidate<'a>>,
    ) -> Option<&'b Candidate<'a>>
    where
        'a: 'b,
    {
        candidates.min_by(|a, b| compare_fatigue(a, b))
    }
}

fn compare_fatigue(a: &Candidate<'_>, b: &Candidate<'_>) -> std::cmp::Ordering {
    a.fatigue(0)
        .total_cmp(&b.fatigue(0))
        .then_with(|| a.player.info.id.cmp(&b.player.info.id))
}
