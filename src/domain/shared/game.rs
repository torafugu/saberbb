use super::game_state::GameState;
use super::player::Player;
use super::team::Team;
use super::types::{BattingResult, InningType};
use crate::t;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use strum_macros::EnumString;

pub const TOTAL_GAMES: u16 = 140;

#[derive(Clone, Serialize, Deserialize, Debug, EnumString)]
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

#[derive(Serialize, Deserialize, Debug)]
pub struct GameSeason {
    // pub start_season: u16,
    pub start_date: NaiveDate,
    // pub current_season: u16,
    // pub current_round_seq: u16,
    pub season: u16,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GameScheduler {
    pub id: u32,
    pub season: u16,
    pub round_seq: u16,
    pub seq: u16,
    pub planned_date: NaiveDate,
    pub away_team: Team,
    pub home_team: Team,
    pub game_type: GameType,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GameHeader {
    pub id: u32,
    pub actual_date: NaiveDate,
    pub away_team: Team,
    pub home_team: Team,
    pub game_type: GameType,
    pub away_points: u8,
    pub home_points: u8,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GameResult {
    pub id: u32,
    pub actual_date: NaiveDate,
    pub innings: Vec<Inning>,
    pub away_points: u8,
    pub home_points: u8,
}
impl GameResult {
    pub fn update_point(&mut self, game_state: &GameState) {
        if game_state.inning_tb == InningType::Bottom {
            self.home_points = game_state.home_total_point;
        } else {
            self.away_points = game_state.away_total_point;
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GameRow {
    pub id: u32,
    pub season: u16,
    pub round_seq: u16,
    pub seq: u16,
    pub planned_date: NaiveDate,
    pub actual_date: NaiveDate,
    pub away_team: Team,
    pub home_team: Team,
    pub game_type: GameType,
    pub innings: Vec<Inning>,
    pub away_points: u8,
    pub home_points: u8,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Inning {
    pub seq: u8,
    pub tb: InningType,
    pub counts: Vec<Count>,
}
impl Inning {
    pub fn is(&self, seq: u8, tb: InningType) -> bool {
        self.seq == seq && self.tb == tb
    }

    pub fn add_count(&mut self, count: Count) {
        self.counts.push(count);
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Count {
    pub seq: u8,
    pub bases_occupied: u8,
    pub pitcher: Arc<Player>,
    pub catcher: Arc<Player>,
    pub first_baseman: Arc<Player>,
    pub second_baseman: Arc<Player>,
    pub third_baseman: Arc<Player>,
    pub shortstop: Arc<Player>,
    pub left_fielder: Arc<Player>,
    pub center_fielder: Arc<Player>,
    pub right_fielder: Arc<Player>,
    pub batter: Arc<Player>,
    pub result: BattingResult,
    pub point: u8,
    pub out: u8,
}
