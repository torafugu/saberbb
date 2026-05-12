use super::player::Player;
use super::team::Team;
use super::types::BattingResult;
use super::types::InningType;
use crate::domain::resolver::batting_resolve;
use crate::domain::shared::types::Position;
use crate::t;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use strum_macros::EnumString;

pub const MAX_INNING: i8 = 9;
pub const MAX_OUT: i8 = 3;
pub const MAX_BATTER_ORDER: i8 = 9;
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
    pub away_point: i8,
    pub home_point: i8,
    pub away_players: Vec<BattingOrder>,
    pub home_players: Vec<BattingOrder>,
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

#[derive(Clone, Debug)]
pub struct GameState {
    pub is_in_game: bool,
    pub inning_seq: i8,
    pub inning_tb: InningType,
    pub away_total_point: i8,
    pub home_total_point: i8,
    pub away_batter_order: i8,
    pub home_batter_order: i8,
}
impl GameState {
    pub fn new() -> GameState {
        GameState {
            is_in_game: true,
            inning_seq: 1,
            inning_tb: InningType::Top,
            away_total_point: 0,
            home_total_point: 0,
            away_batter_order: 1,
            home_batter_order: 1,
        }
    }

    pub fn is_active(&self) -> bool {
        self.is_in_game
    }

    pub fn is_walk_off(&self) -> bool {
        if self.inning_seq == MAX_INNING
            && self.inning_tb == InningType::Bottom
            && self.home_total_point > self.away_total_point
        {
            true
        } else {
            false
        }
    }

    pub fn is_game_set(&self) -> bool {
        if self.inning_seq == MAX_INNING
            && (self.inning_tb == InningType::Bottom
                || self.home_total_point > self.away_total_point)
        {
            true
        } else {
            false
        }
    }

    pub fn new_inning(&mut self) -> Inning {
        if self.inning_tb == InningType::Bottom {
            self.inning_seq += 1;
            self.inning_tb = InningType::Top;
        } else {
            self.inning_tb = InningType::Bottom;
        }
        Inning {
            seq: self.inning_seq,
            tb: self.inning_tb,
            counts: Vec::new(),
            point: 0,
        }
    }

    pub fn batter_order(&self) -> i8 {
        if self.inning_tb == InningType::Top {
            self.away_batter_order
        } else {
            self.home_batter_order
        }
    }

    pub fn update(&mut self, point: i8) {
        if self.inning_tb == InningType::Top {
            self.away_total_point += point;
            if self.away_batter_order == MAX_BATTER_ORDER {
                self.away_batter_order = 1;
            } else {
                self.away_batter_order += 1;
            }
        } else {
            self.home_total_point += point;
            if self.home_batter_order == MAX_BATTER_ORDER {
                self.home_batter_order = 1;
            } else {
                self.home_batter_order += 1;
            }
        }
    }
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

    pub fn add_count(&mut self, count: Count) {
        self.point += count.point;
        self.counts.push(count);
    }
}

#[derive(Clone, Debug)]
pub struct InningState {
    pub count_seq: i8,
    pub bases: Bases,
    pub out: i8,
}
impl InningState {
    pub fn new() -> InningState {
        InningState {
            count_seq: 0,
            bases: Bases::new(),
            out: 0,
        }
    }

    pub fn is_active(&self) -> bool {
        if self.out < MAX_OUT { true } else { false }
    }

    pub fn batting_resolve(&mut self, batter: &Player) -> Count {
        let batting_result = batting_resolve(batter);
        if batting_result == BattingResult::Out {
            self.out += 1;
        }
        let point = self.bases.advance(&batting_result);
        Count {
            seq: self.count_seq,
            bases: self.bases.clone(),
            batter: Arc::new(batter.clone()),
            result: batting_result,
            point: point,
            out: self.out,
        }
    }

    pub fn add_count_seq(&mut self) {
        self.count_seq += 1;
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

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct BattingOrder {
    pub order: i8,
    pub position: Position,
    pub player: Player,
}
