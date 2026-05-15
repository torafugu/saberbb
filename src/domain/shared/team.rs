use super::game_state::BattingOrder;
use super::types::Position;
use rand::prelude::SliceRandom;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::domain::shared::player::Player;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct League {
    pub id: u16,
    pub name: Arc<str>,
    pub teams: Vec<Team>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Team {
    pub id: u16,
    pub name: Arc<str>,
    pub players: Vec<Player>,
}
impl Team {
    pub fn lineup(&mut self, is_dh: bool) -> Vec<BattingOrder> {
        let mut rng = rand::rng();
        let mut order: u8 = 1;
        let mut batting_orders = Vec::new();
        let mut players = self.players.clone();
        players.shuffle(&mut rng);

        if is_dh {
            for position in &Position::ALL {
                if let Some(selection) = players.pop() {
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
                if let Some(selection) = players.pop() {
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
    pub games: u16,
    pub wins: u16,
    pub losses: u16,
    pub draws: u16,
    pub pct: f32,
    pub gb: f32,
    pub r: u16,
    pub ra: u16,
}
