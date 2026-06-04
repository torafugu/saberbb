use super::player::Position;
use crate::domain::shared::game_state::GameError;
use crate::domain::shared::player::Player;
use rand::prelude::IndexedRandom;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use validator::Validate;

pub const MAX_BATTING_ORDER: usize = 9;

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

    pub fn lineup(&mut self, is_dh: bool) -> Result<Lineup, GameError> {
        let mut rng = rand::rng();
        let mut batting_orders = Vec::new();
        // let mut players = self.players.clone();
        // players.shuffle(&mut rng);

        let mut position_map = self.init_position_hashmap(is_dh);
        // TODO: Consider multiptile defense skills
        for player in &self.players {
            position_map
                .entry(player.defensive_skills[0].position.clone())
                .or_insert_with(Vec::new)
                .push(player);
        }

        for (key, vec) in position_map {
            if let Some(&random_player) = vec.choose(&mut rng) {
                batting_orders.push(BattingOrder {
                    index: 0,
                    position: key,
                    player: random_player.clone(),
                });
            } else {
                return Err(GameError::NoPlayerFor(key.to_string()));
            }
        }

        // TODO: The batting order should be considered by team starategy.
        batting_orders.sort_by(|a, b| b.player.mod_ba.total_cmp(&a.player.mod_ba));
        for (order, batting_order) in batting_orders.iter_mut().enumerate() {
            batting_order.index = (order + 1) as u8;
        }

        Ok(Lineup::new(batting_orders)?)
    }

    fn init_position_hashmap(&self, is_dh: bool) -> HashMap<Position, Vec<&Player>> {
        let mut map: HashMap<Position, Vec<&Player>> = HashMap::new();
        if is_dh {
            for position in Position::ALL {
                map.entry(position).or_insert_with(Vec::new);
            }
        } else {
            for position in Position::ALL_NO_DH {
                map.entry(position).or_insert_with(Vec::new);
            }
        }
        map
    }
}

#[derive(Clone, Debug)]
pub struct Lineup {
    pub current_index: usize,
    pub batters: Vec<BattingOrder>,
}
impl Lineup {
    pub fn new(batters: Vec<BattingOrder>) -> Result<Self, GameError> {
        if batters.is_empty() {
            return Err(GameError::Lineup("Batting order is empty".to_string()));
        } else if batters.len() != MAX_BATTING_ORDER {
            return Err(GameError::Lineup(
                "Batting order length is not 9".to_string(),
            ));
        }
        Ok(Self {
            current_index: 0,
            batters,
        })
    }
}
impl Iterator for Lineup {
    type Item = Player;

    fn next(&mut self) -> Option<Self::Item> {
        if self.batters.is_empty() {
            return None;
        }

        let batting_order = self.batters[self.current_index].clone();

        // Use the modulo operator (%) to rotate the index around the range 0..N
        self.current_index = (self.current_index + 1) % self.batters.len();
        Some(batting_order.player)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BattingOrder {
    pub index: u8,
    pub position: Position,
    pub player: Player,
}
impl BattingOrder {
    pub fn is(&self, position: Position) -> bool {
        self.position == position
    }
}
