use super::game::BattingOrder;
use super::types::Position;
use rand::prelude::IndexedRandom;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::domain::shared::player::Player;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct League {
    pub id: i16,
    pub name: Arc<str>,
    pub teams: Vec<Team>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Team {
    pub id: i16,
    pub name: Arc<str>,
    pub players: Vec<Player>,
}
impl Team {
    pub fn lineup(&mut self, is_dh: bool) -> Vec<BattingOrder> {
        let mut rng = rand::rng();
        let mut order: i8 = 1;
        let mut batting_orders = Vec::new();
        let players = self.players.clone();

        if is_dh {
            for position in &Position::ALL {
                if let Some(selection) = players.choose(&mut rng) {
                    batting_orders.push(BattingOrder {
                        order: order,
                        position: position.clone(),
                        player: selection.clone(),
                    });
                }
                order += 1;
            }
        } else {
            for position in &Position::ALL_NO_DH {
                if let Some(selection) = players.choose(&mut rng) {
                    batting_orders.push(BattingOrder {
                        order: order,
                        position: position.clone(),
                        player: selection.clone(),
                    });
                }
                order += 1;
            }
        }

        batting_orders
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Standing {
    pub team: Team,
    pub games: i16,
    pub wins: i16,
    pub losses: i16,
    pub draws: i16,
    pub pct: f32,
    pub gb: f32,
    pub r: i16,
    pub ra: i16,
}
