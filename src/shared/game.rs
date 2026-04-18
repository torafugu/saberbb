use super::player::Batter;
use super::team::Team;
use super::types::BattingResult;
use super::types::InningType;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use strum::{Display, EnumString};

pub const MAX_INNING: i8 = 9;
pub const MAX_OUT: i8 = 3;

#[derive(Serialize, Deserialize, Debug, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum GameType {
    EXHIBITION,
    REGULAR,
    POSTSEASON,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GameManager {
    pub season: i16,
    pub date: i16,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GameSchedule {
    pub season: i16,
    pub seq: i16,
    pub away_team: Team,
    pub home_team: Team,
    pub game_type: GameType,
}

#[derive(Clone)]
pub struct Game {
    pub seq: i32,
    pub top_team: Team,
    pub bottom_team: Team,
    pub innings: Vec<Inning>,
    pub top_batters: Vec<Batter>,
    pub bottom_batters: Vec<Batter>,
}
impl Game {
    pub fn add_inning(&mut self, inning: Inning) {
        self.innings.push(inning);
    }
}

#[derive(Clone)]
pub struct Inning {
    pub tb: InningType,
    pub seq: i8,
    pub counts: Vec<Count>,
    pub point: i8,
}

#[derive(Clone)]
pub struct Count {
    pub seq: i32,
    pub is_first_runner: bool,
    pub is_second_runner: bool,
    pub is_third_runner: bool,
    pub batter: Arc<Batter>,
    pub result: BattingResult,
    pub point: i8,
    pub out: i8,
}
