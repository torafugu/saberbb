use super::game_state::GameState;
use super::player::Player;
use super::team::Team;
use super::types::{Base, BattingResult, InningType};
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
    pub start_season: u16,
    pub start_date: NaiveDate,
    pub current_season: u16,
    pub current_round_seq: u16,
    pub scheduled_season: u16,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GameRound {
    pub id: u32,
    pub season: u16,
    pub seq: u16,
    pub date: NaiveDate,
    pub games: Vec<Game>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Game {
    pub id: u32,
    pub planned_date: NaiveDate,
    pub actual_date: NaiveDate,
    pub away_team: Team,
    pub home_team: Team,
    pub game_type: GameType,
    pub innings: Vec<Inning>,
    pub away_point: u8,
    pub home_point: u8,
}
impl Game {
    pub fn update_point(&mut self, game_state: &GameState) {
        if game_state.inning_tb == InningType::Bottom {
            self.home_point = game_state.home_total_point;
        } else {
            self.away_point = game_state.away_total_point;
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Inning {
    pub seq: u8,
    pub tb: InningType,
    pub counts: Vec<Count>,
    pub point: u8,
}
impl Inning {
    pub fn is(&self, seq: u8, tb: InningType) -> bool {
        self.seq == seq && self.tb == tb
    }

    pub fn add_count(&mut self, count: Count) {
        self.point += count.point;
        self.counts.push(count);
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Count {
    pub seq: u8,
    pub bases_occupied: u8,
    pub batter: Arc<Player>,
    pub result: BattingResult,
    pub point: u8,
    pub out: u8,
}
