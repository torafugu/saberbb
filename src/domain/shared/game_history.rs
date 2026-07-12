use super::game::BattingResult;
use crate::domain::shared::player::{PlayerInfo, Position};
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct ActiveFielderView {
    pub start_count_seq: u16,
    pub end_count_seq: u16,
    pub team_id: u16,
    pub position: Position,
    pub player: PlayerInfo,
}
impl ActiveFielderView {
    pub fn is_position(&self, team_id: u16, position: Position, count_seq: u16) -> bool {
        self.team_id == team_id
            && self.position == position
            && self.start_count_seq <= count_seq
            && self.end_count_seq >= count_seq
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct BattingResultView {
    pub count_seq: u16,
    pub pitcher: PlayerInfo,
    pub batter: PlayerInfo,
    pub result: BattingResult,
}
impl BattingResultView {
    pub fn is(&self, count_seq: u16) -> bool {
        self.count_seq == count_seq
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct ActiveFielderHistory {
    pub start_count_seq: u16,
    pub end_count_seq: u16,
    pub team_id: u16,
    pub position: Position,
    pub player_id: i64,
}
impl ActiveFielderHistory {
    pub fn new(
        start_count_seq: u16,
        end_count_seq: u16,
        team_id: u16,
        position: Position,
        player_id: i64,
    ) -> Self {
        Self {
            start_count_seq: start_count_seq,
            end_count_seq: end_count_seq,
            team_id: team_id,
            position: position,
            player_id: player_id,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct BattingResultHistory {
    pub count_seq: u16,
    pub pitcher_id: i64,
    pub batter_id: i64,
    pub result: BattingResult,
}
impl BattingResultHistory {
    pub fn new() -> Self {
        Self {
            count_seq: 0,
            pitcher_id: 0,
            batter_id: 0,
            result: BattingResult::Out,
        }
    }
}
