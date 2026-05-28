use super::player::Position;
use rand::prelude::SliceRandom;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use validator::Validate;

use crate::domain::shared::player::Player;

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

    pub fn lineup(&mut self, is_dh: bool) -> Vec<Player> {
        let mut rng = rand::rng();
        let mut batting_orders = Vec::new();
        let mut players = self.players.clone();
        players.shuffle(&mut rng);

        if is_dh {
            for _position in &Position::ALL {
                if let Some(selection) = players.pop() {
                    batting_orders.push(selection.clone());
                }
            }
        } else {
            for _position in &Position::ALL_NO_DH {
                if let Some(selection) = players.pop() {
                    batting_orders.push(selection.clone());
                }
            }
        }

        batting_orders
    }
}
