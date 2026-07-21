use super::game::BattingResult;
use crate::domain::resolver::fielding_resolver::PlayType;
use crate::domain::resolver::running_resolver::RunningEvent;
use crate::domain::shared::game_state::Ruling;
use crate::domain::shared::stadium::Base;
use crate::domain::shared::{
    ball::BattedBall,
    player::{PlayerInfo, Position},
};
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameEntryView {
    pub start_count_seq: u16,
    pub end_count_seq: u16,
    pub team_id: u16,
    pub position: Position,
    pub player: PlayerInfo,
}
impl PlayerGameEntryView {
    pub fn is_position(&self, team_id: u16, position: Position, count_seq: u16) -> bool {
        self.team_id == team_id
            && self.position == position
            && self.start_count_seq <= count_seq
            && self.end_count_seq >= count_seq
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameBattingView {
    pub count_seq: u16,
    pub pitcher: PlayerInfo,
    pub batter: PlayerInfo,
    pub ball: BattedBall,
    pub fielder_position: Option<Position>,
    pub result: BattingResult,
}
impl PlayerGameBattingView {
    pub fn is(&self, count_seq: u16) -> bool {
        self.count_seq == count_seq
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameRunningView {
    pub count_seq: u16,
    pub seq: u8,
    pub defense_time: f64,
    pub runner_time: f64,
    pub throw_target_base: Base,
    pub target_runner: Option<PlayerInfo>,
    pub event: RunningEvent,
    pub play_type: PlayType,
    pub ruling: Ruling,
    pub runs_scored: u8,
    pub runner_1st: Option<PlayerInfo>,
    pub runner_2nd: Option<PlayerInfo>,
    pub runner_3rd: Option<PlayerInfo>,
}
impl PlayerGameRunningView {
    pub fn is(&self, count_seq: u16, seq: u8) -> bool {
        self.count_seq == count_seq && self.seq == seq
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameEntry {
    pub start_count_seq: u16,
    pub end_count_seq: Option<u16>,
    pub position: Position,
    pub player_id: i64,
}
impl PlayerGameEntry {
    pub fn new(
        start_count_seq: u16,
        end_count_seq: Option<u16>,
        position: Position,
        player_id: i64,
    ) -> Self {
        Self {
            start_count_seq: start_count_seq,
            end_count_seq: end_count_seq,
            position: position,
            player_id: player_id,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGamePitching {
    pub count_seq: u16,
    pub pitcher_id: i64,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameBatting {
    pub count_seq: u16,
    pub pitcher_id: i64,
    pub batter_id: i64,
    pub ball: BattedBall,
    pub fielder_position: Option<Position>,
    pub result: BattingResult,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameFielding {
    pub count_seq: u16,
    pub seq: u8,
    pub catch_fielder_id: i64,
    pub catch_fielder_position: Position,
    pub cutoff_fielder_id: Option<i64>,
    pub cutoff_fielder_position: Option<Position>,
    pub final_fielder_id: Option<i64>,
    pub final_fielder_position: Option<Position>,
    pub time_to_field: f64,
    pub play_type: PlayType,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameRunning {
    pub count_seq: u16,
    pub seq: u8,
    pub defense_time: f64,
    pub runner_time: f64,
    pub throw_target_base: Base,
    pub target_runner_id: Option<i64>,
    pub event: RunningEvent,
    pub play_type: PlayType,
    pub ruling: Ruling,
    pub runs_scored: u8,
    pub runner_1st_id: Option<i64>,
    pub runner_2nd_id: Option<i64>,
    pub runner_3rd_id: Option<i64>,
}
