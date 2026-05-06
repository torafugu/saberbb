use super::player::Player;
use super::team::Team;
use super::types::BattingResult;
use super::types::InningType;
use crate::t;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use strum_macros::EnumString;

pub const MAX_INNING: i8 = 9;
pub const MAX_OUT: i8 = 3;
pub const TOTAL_GAMES: i16 = 140;

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

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Bases {
    pub first: bool,
    pub second: bool,
    pub third: bool,
}

impl Bases {
    pub fn new() -> Self {
        Self {
            first: false,
            second: false,
            third: false,
        }
    }

    pub fn advance(&mut self, batting_result: &BattingResult) -> i8 {
        let mut point: i8 = 0;

        match batting_result {
            BattingResult::Single => {
                if self.third {
                    point += 1;
                    self.third = false;
                }
                if self.second {
                    self.second = false;
                    self.third = true;
                }
                if self.first {
                    self.second = true;
                }
                self.first = true;
            }
            BattingResult::Double => {
                if self.third {
                    point += 1;
                    self.third = false;
                }
                if self.second {
                    point += 1;
                }
                if self.first {
                    self.first = false;
                    self.third = true;
                }
                self.second = true;
            }
            BattingResult::Triple => {
                if self.third {
                    point += 1;
                }
                if self.second {
                    point += 1;
                    self.second = false;
                }
                if self.first {
                    point += 1;
                    self.first = false;
                }
                self.third = true;
            }
            BattingResult::HomeRun => {
                if self.third {
                    point += 1;
                    self.third = false;
                }
                if self.second {
                    point += 1;
                    self.second = false;
                }
                if self.first {
                    point += 1;
                    self.first = false;
                }
                point += 1;
            }
            _ => {}
        }

        point
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GameSeason {
    pub start_season: i16,
    pub start_date: NaiveDate,
    pub current_season: i16,
    pub current_round_seq: i16,
    pub scheduled_season: i16,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GameRound {
    pub id: i32,
    pub season: i16,
    pub seq: i16,
    pub date: NaiveDate,
    pub games: Vec<Game>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Game {
    pub id: i32,
    pub planned_date: NaiveDate,
    pub actual_date: NaiveDate,
    pub away_team: Team,
    pub home_team: Team,
    pub game_type: GameType,
    pub innings: Vec<Inning>,
    pub away_point: i16,
    pub home_point: i16,
    pub away_batters: Vec<Player>,
    pub home_batters: Vec<Player>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Inning {
    pub seq: i8,
    pub tb: InningType,
    pub counts: Vec<Count>,
    pub point: i8,
}
impl Inning {
    pub fn is(&self, seq: i8, tb: InningType) -> bool {
        self.seq == seq && self.tb == tb
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Count {
    pub seq: i8,
    pub bases: Bases,
    pub batter: Arc<Player>,
    pub result: BattingResult,
    pub point: i8,
    pub out: i8,
}
