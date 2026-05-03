use super::player::Batter;
use super::team::Team;
use super::types::BattingResult;
use super::types::InningType;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use strum::EnumString;

pub const MAX_INNING: i8 = 9;
pub const MAX_OUT: i8 = 3;
pub const TOTAL_GAMES: i16 = 140;
const EXHIBITION: &str = "Exhibition";
const REGULAR: &str = "Regular";
const POSTSEASON: &str = "postseason";

#[derive(Clone, Serialize, Deserialize, Debug, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum GameType {
    EXHIBITION,
    REGULAR,
    POSTSEASON,
}
impl fmt::Display for GameType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            GameType::EXHIBITION => write!(f, "{EXHIBITION}"),
            GameType::REGULAR => write!(f, "{REGULAR}"),
            GameType::POSTSEASON => write!(f, "{POSTSEASON}"),
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
            BattingResult::SINGLE => {
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
            BattingResult::DOUBLE => {
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
            BattingResult::TRIPLE => {
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
            BattingResult::HOMERUN => {
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
    pub date: NaiveDate,
    pub away_team: Team,
    pub home_team: Team,
    pub game_type: GameType,
    pub innings: Vec<Inning>,
    pub away_point: i16,
    pub home_point: i16,
    pub away_batters: Vec<Batter>,
    pub home_batters: Vec<Batter>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Inning {
    pub seq: i8,
    pub tb: InningType,
    pub counts: Vec<Count>,
    pub point: i8,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Count {
    pub seq: i32,
    pub bases: Bases,
    pub batter: Arc<Batter>,
    pub result: BattingResult,
    pub point: i8,
    pub out: i8,
}
