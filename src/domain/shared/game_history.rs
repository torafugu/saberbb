use super::game::TB;
use super::player::Position;
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct BattingOrderHistory {
    pub start_inning_seq: u8,
    pub start_inning_tb: TB,
    pub start_count_seq: u8,
    pub end_inning_seq: Option<u8>,
    pub end_inning_tb: Option<TB>,
    pub end_count_seq: Option<u8>,
    pub team_id: u16,
    pub index: u8,
    pub position: Position,
    pub player_id: u32,
}
impl BattingOrderHistory {
    pub fn new(
        start_inning_seq: u8,
        start_inning_tb: TB,
        start_count_seq: u8,
        team_id: u16,
        index: u8,
        position: Position,
        player_id: u32,
    ) -> Self {
        Self {
            start_inning_seq: start_inning_seq,
            start_inning_tb: start_inning_tb,
            start_count_seq: start_count_seq,
            end_inning_seq: None,
            end_inning_tb: None,
            end_count_seq: None,
            team_id: team_id,
            index: index,
            position: position,
            player_id: player_id,
        }
    }
}
