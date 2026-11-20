//! Select pitchers using pregame workload, in-game pitches and an ability evaluation.
use super::pitcher_evaluation::{PitcherAttributeRating, PitcherAttributeWeights};
use crate::domain::shared::{
    pitcher_fatigue::PitcherFatigue,
    player::{Player, Position},
    team::Team,
};
use std::collections::BTreeMap;

const MAX_ROTATION_SIZE: usize = 5;

struct Candidate<'a> {
    player: &'a Player,
    fatigue: PitcherFatigue,
    ability_score: f64,
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
    ) -> Self {
        let mut candidates: Vec<_> = team
            .players
            .iter()
            .filter(|player| player.defense_skills.position == Position::P)
            .filter_map(|player| {
                let pitcher = player.defense_skills.pitcher.as_ref()?;
                let rating = PitcherAttributeRating::measure(pitcher, base_four_seam_speed);
                Some(Candidate {
                    player,
                    ability_score: rating.map_or(f64::NEG_INFINITY, |rating| weights.score(rating)),
                    fatigue: PitcherFatigue {
                        entering_load: loads.get(&player.info.id).copied().unwrap_or_default(),
                        reference_pitches: PitcherFatigue::reference_pitches(pitcher.stamina),
                        pitches: 0,
                    },
                })
            })
            .collect();

        // Prefer pitchers who can handle the reference workload. If there are
        // too few, fill the remaining slots with the highest-stamina pitchers.
        let starter_workload = PitcherFatigue::reference_pitches(0.0);
        candidates.sort_by(|a, b| {
            let a_capacity = a.fatigue.reference_pitches;
            let b_capacity = b.fatigue.reference_pitches;
            let a_ready = a_capacity >= starter_workload;
            let b_ready = b_capacity >= starter_workload;
            b_ready
                .cmp(&a_ready)
                .then_with(|| {
                    if a_ready && b_ready {
                        b.ability_score
                            .total_cmp(&a.ability_score)
                            .then_with(|| b_capacity.total_cmp(&a_capacity))
                    } else {
                        b_capacity.total_cmp(&a_capacity)
                    }
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
