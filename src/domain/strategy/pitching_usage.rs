//! Select pitchers from the roster using pregame workload and pitches in this game.
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
    pub(crate) fn new(team: &'a Team, loads: &BTreeMap<i64, f64>) -> Self {
        let mut candidates: Vec<_> = team
            .players
            .iter()
            .filter(|player| player.defense_skills.position == Position::P)
            .filter_map(|player| {
                let stamina = player.defense_skills.pitcher.as_ref()?.stamina;
                Some(Candidate {
                    player,
                    fatigue: PitcherFatigue {
                        entering_load: loads.get(&player.info.id).copied().unwrap_or_default(),
                        reference_pitches: PitcherFatigue::reference_pitches(stamina),
                        pitches: 0,
                    },
                })
            })
            .collect();

        // Use stamina to assign roles until rosters have explicit pitcher roles.
        candidates.sort_by(|a, b| {
            b.fatigue
                .reference_pitches
                .total_cmp(&a.fatigue.reference_pitches)
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

        Self::freshest(
            self.candidates[self.rotation_size..]
                .iter()
                .filter(|c| !game_pitches.contains_key(&c.player.info.id) && c.fatigue(0) < 1.0),
        )
        .map(|candidate| candidate.player)
    }

    fn freshest<'b>(
        candidates: impl Iterator<Item = &'b Candidate<'a>>,
    ) -> Option<&'b Candidate<'a>>
    where
        'a: 'b,
    {
        candidates.min_by(|a, b| {
            a.fatigue(0)
                .total_cmp(&b.fatigue(0))
                .then_with(|| a.player.info.id.cmp(&b.player.info.id))
        })
    }
}
