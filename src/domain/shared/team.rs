use super::player::Position;
use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::game_state::{
    ActiveBatter, ActiveCatcher, ActiveFielder, ActivePitcher, ActivePlayer, GameError,
};
use crate::domain::shared::player::Player;
use crate::domain::shared::stadium::MOUND_DISTANCE;
use crate::domain::util::PolarPosition;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use validator::Validate;

pub const MAX_LINEUP_PLAYERS: usize = 10;

#[derive(Serialize, Deserialize, Clone, Debug, Validate)]
pub struct League {
    pub id: u16,
    pub name: Arc<str>,
    pub teams: Vec<Team>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Validate)]
pub struct Team {
    pub id: u16,
    pub name: Arc<str>,
    pub players: Vec<Player>,
}
impl Team {
    pub fn min(id: u16, name: &str) -> Self {
        Self {
            id: id,
            name: name.into(),
            players: Vec::new(),
        }
    }

    pub fn lineup(&mut self, rng: &mut dyn RandomProvider) -> Result<Lineup, GameError> {
        let mut players = Vec::new();

        let mut position_map = self.init_position_hashmap();

        for player in &self.players {
            position_map
                .entry(player.defense_skills.position)
                .or_insert_with(Vec::new)
                .push(player);
        }

        for (key, vec) in position_map {
            if vec.is_empty() {
                return Err(GameError::NoPlayerFor(key.to_string()));
            }

            // TODO: Consider player ability to select the lineup
            let index = rng.gen_range(0, vec.len() - 1);
            let random_player = vec[index];
            let position = random_player.defense_skills.position;

            let (fielding_position, fielder, polar_position) = if position != Position::DH {
                // TODO: Move to team pameter
                let polar_position = match position {
                    Position::P => PolarPosition::new(MOUND_DISTANCE, 0.0),
                    Position::C => PolarPosition::new(0.0, 0.0),
                    Position::FB => PolarPosition::new(35.0, 33.0),
                    Position::SB => PolarPosition::new(40.0, 18.0),
                    Position::TB => PolarPosition::new(35.0, -33.0),
                    Position::SS => PolarPosition::new(40.0, -18.0),
                    Position::RF => PolarPosition::new(80.0, 26.0),
                    Position::CF => PolarPosition::new(90.0, 0.0),
                    Position::LF => PolarPosition::new(80.0, 80.0),
                    Position::DH => {
                        return Err(GameError::Lineup("No polar position for DH".to_string()));
                    }
                };

                (
                    Some(position),
                    Some(random_player.fielder()?),
                    Some(polar_position),
                )
            } else {
                (None, None, None)
            };

            players.push(ActivePlayer {
                id: random_player.info.id,
                batting_order: None,
                batter: if position != Position::P {
                    Some(random_player.batter()?)
                } else {
                    None
                },
                runner: random_player.runner(),
                fielding_position,
                fielder,
                polar_position,
                pitcher: if position == Position::P {
                    Some(random_player.pitcher()?)
                } else {
                    None
                },
                catcher: if position == Position::C {
                    Some(random_player.catcher()?)
                } else {
                    None
                },
            });
        }

        // TODO: The batting order should be considered by team starategy.
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

        Ok(Lineup::new(players)?)
    }

    fn init_position_hashmap(&self) -> HashMap<Position, Vec<&Player>> {
        let mut map: HashMap<Position, Vec<&Player>> = HashMap::new();
        for position in Position::ALL {
            map.entry(position).or_insert_with(Vec::new);
        }
        map
    }
}

#[derive(Clone, Debug)]
pub struct Lineup {
    pub current_index: usize,
    pub players: [ActivePlayer; MAX_LINEUP_PLAYERS],
}
impl Lineup {
    pub fn new(vec_players: Vec<ActivePlayer>) -> Result<Self, GameError> {
        let arr_players: [ActivePlayer; MAX_LINEUP_PLAYERS] = vec_players
            .try_into()
            .expect("Number of lineup players must be 10.");

        let lineup = Self {
            current_index: 0,
            players: arr_players,
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
        lineup.pitcher();
        lineup.catcher();

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
            .filter_map(ActivePlayer::active_fielder)
            .collect()
    }

    pub fn pitcher(&self) -> ActivePitcher {
        self.players
            .iter()
            .find_map(ActivePlayer::active_pitcher)
            .expect("Lineup must have pitcher.")
    }

    pub fn catcher(&self) -> ActiveCatcher {
        self.players
            .iter()
            .find_map(ActivePlayer::active_catcher)
            .expect("Lineup must have catcher.")
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
