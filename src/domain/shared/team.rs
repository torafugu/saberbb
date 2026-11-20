use super::player::Position;
use crate::domain::random_provider::RandomProvider;
use crate::domain::resolver::fielding_physics::FielderRiskTolerance;
use crate::domain::shared::game_state::{
    ActiveBatter, ActiveCatcher, ActiveFielder, ActivePitcher, ActivePlayer, GameError,
};
use crate::domain::shared::player::Player;
use crate::domain::shared::stadium::MOUND_DISTANCE;
use crate::domain::strategy::position_player_usage::lineup_score;
use crate::domain::util::PolarPosition;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use validator::Validate;

pub const MAX_LINEUP_PLAYERS: usize = 10;

#[derive(Serialize, Deserialize, Clone, Debug, Validate)]
pub struct League {
    pub id: u16,
    pub name: Arc<str>,
    pub teams: Vec<Team>,
    pub number_of_games: Option<u16>,
    pub max_inning: Option<u8>,
    pub base_four_seam_speed: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Validate)]
pub struct Team {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manager: Option<super::batting_personnel::Manager>,
    pub id: u16,
    pub name: Arc<str>,
    pub players: Vec<Player>,
}
impl Team {
    pub fn min(id: u16, name: &str) -> Self {
        Self {
            manager: None,
            id: id,
            name: name.into(),
            players: Vec::new(),
        }
    }

    pub fn lineup(&mut self, rng: &mut dyn RandomProvider) -> Result<Lineup, GameError> {
        self.select_lineup(rng, None, None)
    }

    pub(crate) fn select_lineup(
        &self,
        rng: &mut dyn RandomProvider,
        starter: Option<i64>,
        fatigue: Option<&BTreeMap<i64, f64>>,
    ) -> Result<Lineup, GameError> {
        let mut players = Vec::new();

        let mut position_map = self.init_position_hashmap();

        for player in &self.players {
            position_map
                .entry(player.defense_skills.position)
                .or_insert_with(Vec::new)
                .push(player);
        }

        let mut selected_player_ids = Vec::new();

        for position in Position::ALL
            .into_iter()
            .filter(|position| *position != Position::DH)
        {
            let vec = position_map.remove(&position).unwrap_or_default();
            if vec.is_empty() {
                return Err(GameError::NoPlayerFor(position.to_string()));
            }

            let selected = match starter.filter(|_| position == Position::P) {
                Some(id) => vec
                    .iter()
                    .find(|player| player.info.id == id)
                    .copied()
                    .ok_or_else(|| {
                        GameError::Lineup(format!("Starting pitcher {id} is not on the roster"))
                    })?,
                None => select_player(&vec, position, fatigue, rng),
            };
            selected_player_ids.push(selected.info.id);

            players.push(player_in_role(selected, position, None)?);
        }

        let dedicated_dh_candidates = self
            .players
            .iter()
            .filter(|player| player.defense_skills.position == Position::DH)
            .collect::<Vec<_>>();
        let bench_batter_candidates = self
            .players
            .iter()
            .filter(|player| !selected_player_ids.contains(&player.info.id))
            .filter(|player| player.defense_skills.position != Position::P)
            .filter(|player| player.offense_skills.batter.is_some())
            .collect::<Vec<_>>();
        let dh_candidates = if fatigue.is_some() || dedicated_dh_candidates.is_empty() {
            bench_batter_candidates
        } else {
            dedicated_dh_candidates
        };

        if dh_candidates.is_empty() {
            return Err(GameError::NoPlayerFor(Position::DH.to_string()));
        }
        let dh = select_player(&dh_candidates, Position::DH, fatigue, rng);
        players.push(player_in_role(dh, Position::DH, None)?);

        // Batting order continues to use swing speed.
        let mut batter_ids: Vec<(i64, f64)> = players
            .iter()
            .filter_map(|player| Some((player.id, player.batter.as_ref()?.swing_speed)))
            .collect();
        batter_ids.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (order, (player_id, _)) in batter_ids.iter().enumerate() {
            if let Some(player) = players.iter_mut().find(|player| player.id == *player_id) {
                player.batting_order = Some((order + 1) as u8);
            }
        }

        Lineup::new(players)
    }

    fn init_position_hashmap(&self) -> HashMap<Position, Vec<&Player>> {
        let mut map: HashMap<Position, Vec<&Player>> = HashMap::new();
        for position in Position::ALL {
            map.entry(position).or_insert_with(Vec::new);
        }
        map
    }
}

fn select_player<'a>(
    candidates: &[&'a Player],
    role: Position,
    fatigue: Option<&BTreeMap<i64, f64>>,
    rng: &mut dyn RandomProvider,
) -> &'a Player {
    if let Some(loads) = fatigue.filter(|_| role != Position::P) {
        let score = |player: &Player| {
            lineup_score(
                player,
                role,
                loads.get(&player.info.id).copied().unwrap_or_default(),
            )
        };
        return candidates
            .iter()
            .copied()
            .max_by(|a, b| {
                score(a)
                    .total_cmp(&score(b))
                    .then_with(|| b.info.id.cmp(&a.info.id))
            })
            .expect("lineup candidates were checked");
    }
    candidates[rng.gen_range(0, candidates.len() - 1)]
}

fn player_in_role(
    player: &Player,
    position: Position,
    batting_order: Option<u8>,
) -> Result<ActivePlayer, GameError> {
    let polar_position = match position {
        Position::P => PolarPosition::new(MOUND_DISTANCE, 0.0),
        Position::C => PolarPosition::new(0.0, 0.0),
        Position::FB => PolarPosition::new(35.0, 33.0),
        Position::SB => PolarPosition::new(40.0, 18.0),
        Position::TB => PolarPosition::new(35.0, -33.0),
        Position::SS => PolarPosition::new(40.0, -18.0),
        Position::RF => PolarPosition::new(80.0, 26.0),
        Position::CF => PolarPosition::new(90.0, 0.0),
        Position::LF => PolarPosition::new(80.0, -26.0),
        Position::DH | Position::PH => {
            return Ok(ActivePlayer {
                id: player.info.id,
                batting_order,
                batter: Some(player.batter()?),
                runner: player.runner(),
                fielding_position: None,
                fielder: None,
                polar_position: None,
                pitcher: None,
                catcher: None,
            });
        }
    };

    Ok(ActivePlayer {
        id: player.info.id,
        batting_order,
        batter: if position != Position::P {
            Some(player.batter()?)
        } else {
            None
        },
        runner: player.runner(),
        fielding_position: Some(position),
        fielder: Some(player.fielder()?),
        polar_position: Some(polar_position),
        pitcher: if position == Position::P {
            Some(player.pitcher()?)
        } else {
            None
        },
        catcher: if position == Position::C {
            Some(player.catcher()?)
        } else {
            None
        },
    })
}

#[derive(Clone, Debug)]
pub struct Lineup {
    pub current_index: usize,
    pub players: [ActivePlayer; MAX_LINEUP_PLAYERS],
    pub risk_tolerance: FielderRiskTolerance,
}
impl Lineup {
    pub fn new(vec_players: Vec<ActivePlayer>) -> Result<Self, GameError> {
        let arr_players: [ActivePlayer; MAX_LINEUP_PLAYERS] =
            vec_players
                .try_into()
                .map_err(|players: Vec<ActivePlayer>| {
                    GameError::Lineup(format!(
                        "Number of lineup players must be {MAX_LINEUP_PLAYERS}, but got {}.",
                        players.len()
                    ))
                })?;

        let lineup = Self {
            current_index: 0,
            players: arr_players,
            risk_tolerance: FielderRiskTolerance::Balanced,
        };
        if lineup.batters().len() != 9 {
            return Err(GameError::Lineup(
                "Number of batters must be 9.".to_string(),
            ));
        }
        if lineup.fielders().len() != 9 {
            return Err(GameError::Lineup(
                "Number of fielders must be 9.".to_string(),
            ));
        }
        lineup.pitcher()?;
        lineup.catcher()?;

        Ok(lineup)
    }

    pub fn batters(&self) -> Vec<ActiveBatter> {
        let mut batters: Vec<ActiveBatter> = self
            .players
            .iter()
            .filter_map(ActivePlayer::active_batter)
            .collect();
        batters.sort_by_key(|batter| batter.index);
        batters
    }

    pub fn fielders(&self) -> Vec<ActiveFielder> {
        self.players
            .iter()
            .filter_map(|player| player.active_fielder(self.risk_tolerance))
            .collect()
    }

    pub fn change_risk_tolerance(&mut self, risk_tolerance: FielderRiskTolerance) {
        self.risk_tolerance = risk_tolerance;
    }

    pub fn pitcher(&self) -> Result<ActivePitcher, GameError> {
        self.players
            .iter()
            .find_map(ActivePlayer::active_pitcher)
            .ok_or_else(|| GameError::Lineup("Lineup must have pitcher.".to_string()))
    }

    pub(crate) fn replace_pitcher(&mut self, player: &Player) -> Result<i64, GameError> {
        let slot = self
            .players
            .iter_mut()
            .find(|entry| entry.fielding_position == Some(Position::P))
            .ok_or_else(|| GameError::Lineup("Lineup must have pitcher.".to_string()))?;
        let outgoing = slot.id;
        *slot = ActivePlayer {
            id: player.info.id,
            runner: player.runner(),
            fielder: Some(player.fielder()?),
            pitcher: Some(player.pitcher()?),
            ..slot.clone()
        };
        Ok(outgoing)
    }

    pub fn catcher(&self) -> Result<ActiveCatcher, GameError> {
        self.players
            .iter()
            .find_map(ActivePlayer::active_catcher)
            .ok_or_else(|| GameError::Lineup("Lineup must have catcher.".to_string()))
    }

    pub(crate) fn next_position_player(&self) -> Option<&ActivePlayer> {
        let order = self.current_index + 1;
        self.players
            .iter()
            .find(|p| p.batting_order.map(usize::from) == Some(order))
    }

    pub(crate) fn replace_position_player(
        &mut self,
        outgoing: i64,
        replacement: &Player,
        role: Position,
    ) -> Result<u8, GameError> {
        if !matches!(role, Position::DH | Position::PH)
            && replacement.defense_skills.position != role
        {
            return Err(GameError::Lineup(format!(
                "Player {} cannot play {role}",
                replacement.info.id
            )));
        }
        let slot = self
            .players
            .iter_mut()
            .find(|p| p.id == outgoing && p.is_batter())
            .ok_or_else(|| GameError::Lineup(format!("No batting slot for {outgoing}")))?;
        let order = slot.batting_order.expect("batting slot was checked");
        *slot = player_in_role(replacement, role, Some(order))?;
        Ok(order)
    }

    pub fn next(&mut self) -> Result<ActiveBatter, GameError> {
        let batters = self.batters();
        if batters.is_empty() {
            return Err(GameError::Lineup("batters are empty.".to_string()));
        }

        let active_batter = batters[self.current_index].clone();
        self.current_index = (self.current_index + 1) % batters.len();

        Ok(active_batter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::random_provider::FixedRng;
    use crate::domain::test_support::player;

    fn team_with_positions(positions: Vec<Position>) -> Team {
        Team {
            manager: None,
            id: 1,
            name: "Test".into(),
            players: positions
                .into_iter()
                .enumerate()
                .map(|(index, position)| player(index as i64 + 1, position, None))
                .collect(),
        }
    }

    #[test]
    fn lineup_prefers_dedicated_dh() {
        let mut team = team_with_positions(vec![
            Position::P,
            Position::C,
            Position::FB,
            Position::SB,
            Position::TB,
            Position::SS,
            Position::LF,
            Position::CF,
            Position::RF,
            Position::DH,
            Position::RF,
        ]);
        let mut rng = FixedRng::new(0.0);

        let lineup = team.lineup(&mut rng).unwrap();
        let dh = lineup
            .players
            .iter()
            .find(|player| player.fielding_position.is_none())
            .unwrap();

        assert_eq!(dh.id, 10);
        assert_eq!(lineup.batters().len(), 9);
        assert_eq!(lineup.fielders().len(), 9);
    }

    #[test]
    fn lineup_uses_bench_batter_when_dedicated_dh_is_missing() {
        let mut team = team_with_positions(vec![
            Position::P,
            Position::C,
            Position::FB,
            Position::SB,
            Position::TB,
            Position::SS,
            Position::LF,
            Position::CF,
            Position::RF,
            Position::RF,
        ]);
        let mut rng = FixedRng::new(0.0);

        let lineup = team.lineup(&mut rng).unwrap();
        let dh = lineup
            .players
            .iter()
            .find(|player| player.fielding_position.is_none())
            .unwrap();

        assert_eq!(dh.id, 10);
        assert_eq!(lineup.batters().len(), 9);
        assert_eq!(lineup.fielders().len(), 9);
    }
}
