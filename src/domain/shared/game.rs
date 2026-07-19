use super::game_state::ActiveFielder;
use super::game_stats::{
    PlayerGameBatting, PlayerGameBattingView, PlayerGameEntry, PlayerGameEntryView,
    PlayerGameFielding, PlayerGamePitching, PlayerGameRunning,
};
use super::team::Team;
use crate::domain::resolver::fielding_resolver::{
    DefensePlayResult, DoublePlayDefensePlayResult, PlayType,
};
use crate::domain::resolver::running_resolver::{
    DoublePlayRunnerAdvanceResult, RunnerAdvanceResult, RunnersUnsaved, StealRunnerAdvanceResult,
};
use crate::domain::shared::ball::BattedBall;
use crate::domain::shared::player::Position;
use crate::domain::shared::stadium::{Base, Stadium};
use crate::t;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;
use strum_macros::{AsRefStr, EnumString};
use validator::Validate;

pub const BASE_DISTANCE: f64 = 27.431;
// TODO: Move to league wise parameter.
pub const TOTAL_GAMES: u16 = 140;

#[derive(Clone, Serialize, Deserialize, Debug, EnumString, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum GameType {
    Exhibition,
    Regular,
    Postseason,
}
impl fmt::Display for GameType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            GameType::Exhibition => write!(f, "{}", t!("exhibition")),
            GameType::Regular => write!(f, "{}", t!("regular")),
            GameType::Postseason => write!(f, "{}", t!("postseason")),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, EnumString, Serialize, Deserialize, Debug, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum TB {
    Top,
    Bottom,
}
impl std::fmt::Display for TB {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TB::Top => write!(f, "{}", t!("inning_top")),
            TB::Bottom => write!(f, "{}", t!("inning_bottom")),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Validate)]
pub struct GameSeason {
    pub start_date: NaiveDate,
    pub season: u16,
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct GameSchedule {
    pub id: u32,
    pub season: u16,
    pub round_seq: u16,
    pub seq: u16,
    pub planned_date: NaiveDate,
    pub away_team: Team,
    pub home_team: Team,
    pub game_type: GameType,
    pub stadium: Stadium,
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct GameHeader {
    pub id: u32,
    pub actual_date: NaiveDate,
    pub away_team: Team,
    pub home_team: Team,
    pub game_type: GameType,
    pub away_points: u8,
    pub home_points: u8,
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct GameResult {
    pub id: u32,
    pub actual_date: NaiveDate,
    pub innings: Vec<Inning>,
    pub away_total_point: u8,
    pub home_total_point: u8,
    pub player_entries: Vec<PlayerGameEntry>,
    pub player_pitchings: Vec<PlayerGamePitching>,
    pub player_battings: Vec<PlayerGameBatting>,
    pub player_fieldings: Vec<PlayerGameFielding>,
    pub player_runnings: Vec<PlayerGameRunning>,
}
impl GameResult {
    pub fn new(
        id: u32,
        actual_date: NaiveDate,
        away_fielders: &[ActiveFielder; 9],
        home_fielders: &[ActiveFielder; 9],
    ) -> Self {
        let player_entries = Self::init_player_game_entries(away_fielders, home_fielders);
        Self {
            id: id,
            actual_date: actual_date,
            innings: Vec::new(),
            away_total_point: 0,
            home_total_point: 0,
            player_entries,
            player_pitchings: Vec::new(),
            player_battings: Vec::new(),
            player_fieldings: Vec::new(),
            player_runnings: Vec::new(),
        }
    }

    fn init_player_game_entries(
        away_team_players: &[ActiveFielder; 9],
        home_team_players: &[ActiveFielder; 9],
    ) -> Vec<PlayerGameEntry> {
        let mut fielder_records = Vec::new();

        for away_team_player in away_team_players {
            fielder_records.push(Self::add_player_game_entry(1, 1, away_team_player));
        }
        for home_team_player in home_team_players {
            fielder_records.push(Self::add_player_game_entry(1, 1, home_team_player));
        }

        fielder_records
    }

    fn add_player_game_entry(
        start_count_seq: u16,
        end_count_seq: u16,
        active_fielder: &ActiveFielder,
    ) -> PlayerGameEntry {
        PlayerGameEntry::new(
            start_count_seq,
            end_count_seq,
            active_fielder.position,
            active_fielder.id,
        )
    }

    pub fn add_player_batting(
        &mut self,
        count_seq: u16,
        pitcher_id: i64,
        batter_id: i64,
        ball: BattedBall,
        fielder_position: Option<Position>,
        batting_result: BattingResult,
    ) {
        let player_batting = PlayerGameBatting {
            count_seq: count_seq,
            pitcher_id: pitcher_id,
            batter_id: batter_id,
            ball: ball,
            fielder_position: fielder_position,
            result: batting_result,
        };
        self.player_battings.push(player_batting);
    }

    pub fn add_player_fielding_from_double_play(
        &mut self,
        count_seq: u16,
        result: &DoublePlayDefensePlayResult,
    ) {
        let player_fielding = PlayerGameFielding {
            count_seq: count_seq,
            seq: 2,
            catch_fielder_id: result.thrower_fielder_id,
            catch_fielder_position: result.thrower_fielder_position,
            cutoff_fielder_id: None,
            cutoff_fielder_position: None,
            final_fielder_id: Some(result.final_fielder_id),
            final_fielder_position: Some(result.final_fielder_position),
            time_to_field: 0.0,
            play_type: PlayType::ForcePlay,
        };

        self.player_fieldings.push(player_fielding);
    }

    pub fn add_player_fielding(&mut self, count_seq: u16, result: &DefensePlayResult) {
        let player_fielding = PlayerGameFielding {
            count_seq: count_seq,
            seq: 1,
            catch_fielder_id: result.final_fielder_id,
            catch_fielder_position: result.final_fielder_position,
            cutoff_fielder_id: result.cutoff_fielder_id,
            cutoff_fielder_position: result.cutoff_fielder_position,
            final_fielder_id: Some(result.final_fielder_id),
            final_fielder_position: Some(result.final_fielder_position),
            time_to_field: result.time_to_field,
            play_type: result.play_type,
        };

        self.player_fieldings.push(player_fielding);
    }

    pub fn add_player_fielding_from_fly_catch(
        &mut self,
        count_seq: u16,
        fielder_id: i64,
        fielder_position: Position,
        time_to_field: f64,
        play_type: PlayType,
    ) {
        let player_fielding = PlayerGameFielding {
            count_seq: count_seq,
            seq: 1,
            catch_fielder_id: fielder_id,
            catch_fielder_position: fielder_position,
            cutoff_fielder_id: None,
            cutoff_fielder_position: None,
            final_fielder_id: None,
            final_fielder_position: None,
            time_to_field: time_to_field,
            play_type: play_type,
        };

        self.player_fieldings.push(player_fielding);
    }

    pub fn add_player_running_from_double_play(
        &mut self,
        count_seq: u16,
        seq: u8,
        result: &DoublePlayRunnerAdvanceResult,
    ) {
        let player_running = PlayerGameRunning {
            count_seq: count_seq,
            seq: seq,
            defense_time: result.defense_time,
            runner_time: result.runner_time,
            throw_target_base: result.throw_target_base,
            play_type: PlayType::TouchPlay,
            ruling: result.ruling,
            runs_scored: 0,
            runner_1st_id: result.unsaved_runners.runner_id_on(Base::First),
            runner_2nd_id: result.unsaved_runners.runner_id_on(Base::Second),
            runner_3rd_id: result.unsaved_runners.runner_id_on(Base::Third),
        };
        self.player_runnings.push(player_running);
    }

    pub fn add_player_running_from_stolen_base(
        &mut self,
        count_seq: u16,
        runners: RunnersUnsaved,
        result: &StealRunnerAdvanceResult,
    ) {
        let player_running = PlayerGameRunning {
            count_seq: count_seq,
            seq: 1,
            defense_time: result.defense_time,
            runner_time: result.runner_time,
            throw_target_base: result.throw_target_base,
            play_type: PlayType::TouchPlay,
            ruling: result.ruling,
            runs_scored: 0,
            runner_1st_id: runners.runner_id_on(Base::First),
            runner_2nd_id: runners.runner_id_on(Base::Second),
            runner_3rd_id: runners.runner_id_on(Base::Third),
        };
        self.player_runnings.push(player_running);
    }

    pub fn add_player_running(&mut self, count_seq: u16, seq: u8, result: &RunnerAdvanceResult) {
        let player_running = PlayerGameRunning {
            count_seq: count_seq,
            seq: seq,
            defense_time: result.defense_time,
            runner_time: result.runner_time,
            throw_target_base: result.throw_target_base,
            play_type: result.play_type,
            ruling: result.ruling,
            runs_scored: result.runs_scored,
            runner_1st_id: result.unsaved_runners.runner_id_on(Base::First),
            runner_2nd_id: result.unsaved_runners.runner_id_on(Base::Second),
            runner_3rd_id: result.unsaved_runners.runner_id_on(Base::Third),
        };
        self.player_runnings.push(player_running);
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct GameDetail {
    pub id: u32,
    pub actual_date: NaiveDate,
    pub away_team: Team,
    pub home_team: Team,
    pub game_type: GameType,
    pub innings: Vec<Inning>,
    pub away_points: u8,
    pub home_points: u8,
    pub player_entries: Vec<PlayerGameEntryView>,
    pub player_battings: Vec<PlayerGameBattingView>,
    pub player_runnings: Vec<PlayerGameRunning>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct Inning {
    pub seq: u8,
    pub tb: TB,
    pub counts: Vec<Count>,
}
impl Inning {
    pub fn new() -> Self {
        Self {
            seq: 0,
            tb: TB::Bottom,
            counts: Vec::new(),
        }
    }

    pub fn is(&self, seq: u8, tb: TB) -> bool {
        self.seq == seq && self.tb == tb
    }

    pub fn add_count(&mut self, count: Count) {
        self.counts.push(count);
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct Count {
    pub seq: u16,
    pub point: u8,
    pub ball: u8,
    pub strike: u8,
    pub out: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, EnumString, Serialize, Deserialize, Debug, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum BattingResult {
    Single,
    Double,
    Triple,
    HomeRun,
    Foul,
    FieldersChoice,
    Out,
    DoublePlay,
}
impl std::fmt::Display for BattingResult {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BattingResult::Single => write!(f, "{}", t!("single")),
            BattingResult::Double => write!(f, "{}", t!("double")),
            BattingResult::Triple => write!(f, "{}", t!("triple")),
            BattingResult::HomeRun => write!(f, "{}", t!("homerun")),
            BattingResult::Foul => write!(f, "{}", t!("foul")),
            BattingResult::FieldersChoice => write!(f, "{}", t!("fielders_choice")),
            BattingResult::Out => write!(f, "{}", t!("out")),
            BattingResult::DoublePlay => write!(f, "{}", t!("double_play")),
        }
    }
}
impl BattingResult {
    pub fn is_out(&self) -> bool {
        matches!(self, BattingResult::Out)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseCode {
    First = 0,
    Second = 1,
    Third = 2,
}

#[derive(Clone, Copy, PartialEq, Eq, EnumString, Serialize, Deserialize, Debug, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum FieldingResult {
    Success,
    FieldersChoice,
    Error,
}
