use super::game::{BattingResult, TB};
use super::player::{Player, Position};
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
    pub player: Player,
}
impl BattingOrderHistory {
    pub fn new(
        start_inning_seq: u8,
        start_inning_tb: TB,
        start_count_seq: u8,
        end_inning_seq: Option<u8>,
        end_inning_tb: Option<TB>,
        end_count_seq: Option<u8>,
        team_id: u16,
        index: u8,
        position: Position,
        player: Player,
    ) -> Self {
        Self {
            start_inning_seq: start_inning_seq,
            start_inning_tb: start_inning_tb,
            start_count_seq: start_count_seq,
            end_inning_seq: end_inning_seq,
            end_inning_tb: end_inning_tb,
            end_count_seq: end_count_seq,
            team_id: team_id,
            index: index,
            position: position,
            player: player,
        }
    }

    pub fn is_position(
        &self,
        team_id: u16,
        position: Position,
        inning_seq: u8,
        count_seq: u8,
    ) -> bool {
        self.team_id == team_id
            && self.position == position
            && self.start_inning_seq <= inning_seq
            && self.start_count_seq <= count_seq
            && self.end_inning_seq() >= inning_seq
            && self.end_count_seq() >= count_seq
    }

    fn end_inning_seq(&self) -> u8 {
        if let Some(inning_seq) = self.end_inning_seq {
            inning_seq
        } else {
            u8::MAX
        }
    }

    fn end_count_seq(&self) -> u8 {
        if let Some(count_seq) = self.end_count_seq {
            count_seq
        } else {
            u8::MAX
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct BattingResultHistory {
    pub inning_seq: u8,
    pub inning_tb: TB,
    pub count_seq: u8,
    pub team_id: u16,
    pub pitcher: Player,
    pub batter: Player,
    pub result: BattingResult,
}
impl BattingResultHistory {
    pub fn is(&self, team_id: u16, inning_seq: u8, count_seq: u8) -> bool {
        self.team_id == team_id && self.inning_seq == inning_seq && self.count_seq == count_seq
    }
}
