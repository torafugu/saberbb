use crate::domain::shared::{
    game::GameResult,
    game_state::ActivePlayer,
    player::{Player, Position},
    team::{Lineup, Team},
};
use crate::domain::util::sigmoid;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Load accumulated during one game, on the same scale as pregame fatigue.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct PositionPlayerWorkload {
    pub fielding: f64,
    pub running: f64,
    pub batting: f64,
}

impl PositionPlayerWorkload {
    pub fn total(self) -> f64 {
        self.fielding + self.running + self.batting
    }

    pub(crate) fn remaining(self, elapsed_days: i64) -> f64 {
        self.total() * 0.45_f64.powf(elapsed_days as f64)
    }
}

pub(crate) fn lineup_score(player: &Player, position: Position, fatigue: f64) -> f64 {
    let bat = batting_score(player);
    if position == Position::DH {
        return bat - 0.25 * fatigue;
    }
    let Ok(field) = player.fielder() else {
        return f64::NEG_INFINITY;
    };
    let defense = (sigmoid(field.catching)
        + sigmoid((field.running_speed - 7.0) / 0.75)
        + sigmoid((0.5 - field.reaction) / 0.15)
        + sigmoid((field.throw_speed - 35.0) / 5.0)
        + sigmoid((0.65 - field.prep_time) / 0.15))
        / 5.0;
    let defensive_weight = if position == Position::C { 0.55 } else { 0.40 };
    (1.0 - defensive_weight) * bat + defensive_weight * defense - 0.45 * fatigue
}

fn batting_score(player: &Player) -> f64 {
    player
        .batter()
        .map_or(f64::NEG_INFINITY, |batter| batter.score())
}

pub(crate) struct PinchHit<'a> {
    pub hitter: &'a Player,
    pub defender: &'a Player,
    pub position: Position,
}

pub(crate) struct PositionPlayers<'a> {
    pub team: &'a Team,
    pub entering: &'a BTreeMap<i64, f64>,
    pub pending: Option<PinchHit<'a>>,
}

impl<'a> PositionPlayers<'a> {
    fn fatigue(&self, id: i64, game: &GameResult) -> f64 {
        self.entering.get(&id).copied().unwrap_or_default()
            + game
                .position_player_workloads
                .get(&id)
                .copied()
                .unwrap_or_default()
                .total()
    }

    fn bench(&self, game: &GameResult) -> impl Iterator<Item = &'a Player> {
        self.team.players.iter().filter(|p| {
            p.defense_skills.position != Position::P
                && p.offense_skills.batter.is_some()
                && !game
                    .player_entries
                    .iter()
                    .any(|entry| entry.player_id == p.info.id)
        })
    }

    fn best_replacement(
        &self,
        game: &GameResult,
        position: Position,
        exclude: i64,
    ) -> Option<&'a Player> {
        self.bench(game)
            .filter(|p| p.info.id != exclude)
            .filter(|p| position == Position::DH || p.defense_skills.position == position)
            .filter(|p| self.fatigue(p.info.id, game) < 0.65)
            .max_by(|a, b| {
                lineup_score(a, position, self.fatigue(a.info.id, game))
                    .total_cmp(&lineup_score(b, position, self.fatigue(b.info.id, game)))
                    .then_with(|| b.info.id.cmp(&a.info.id))
            })
    }

    pub fn defensive_change(
        &self,
        game: &GameResult,
        lineup: &Lineup,
    ) -> Option<(i64, &'a Player, Position)> {
        lineup
            .players
            .iter()
            .filter_map(|active| {
                let position = active.fielding_position?;
                let fatigue = self.fatigue(active.id, game);
                if position == Position::P || fatigue < 0.80 {
                    return None;
                }
                let outgoing = self.team.players.iter().find(|p| p.info.id == active.id)?;
                let replacement = self.best_replacement(game, position, active.id)?;
                let incoming_fatigue = self.fatigue(replacement.info.id, game);
                let gain = lineup_score(replacement, position, incoming_fatigue)
                    - lineup_score(outgoing, position, fatigue);
                (incoming_fatigue + 0.20 < fatigue && gain > 0.04).then_some((
                    active.id,
                    replacement,
                    position,
                    gain,
                ))
            })
            .max_by(|a, b| a.3.total_cmp(&b.3))
            .map(|(id, replacement, position, _)| (id, replacement, position))
    }

    pub fn pinch_hit(&self, game: &GameResult, active: &ActivePlayer) -> Option<PinchHit<'a>> {
        let outgoing = self.team.players.iter().find(|p| p.info.id == active.id)?;
        let position = active.fielding_position.unwrap_or(Position::DH);
        let score = |p: &Player| batting_score(p) - 0.12 * self.fatigue(p.info.id, game);
        let hitter = self
            .bench(game)
            .filter(|p| self.fatigue(p.info.id, game) < 1.0)
            .filter(|p| score(p) > score(outgoing) + 0.05)
            // A hitter can be rested from defense only if another unused player can take the slot.
            .filter_map(|hitter| {
                let defender = self.best_replacement(game, position, hitter.info.id)?;
                Some(PinchHit {
                    hitter,
                    defender,
                    position,
                })
            })
            .max_by(|a, b| {
                score(a.hitter)
                    .total_cmp(&score(b.hitter))
                    .then_with(|| b.hitter.info.id.cmp(&a.hitter.info.id))
            });
        hitter
    }
}
