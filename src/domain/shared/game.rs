use super::game_history::BattingOrderHistory;
use super::game_state::GameState;
use super::team::{BattingOrder, Team};
use crate::t;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;
use strum_macros::{AsRefStr, EnumString};
use validator::Validate;

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
    pub away_points: u8,
    pub home_points: u8,
    pub batting_order_histories: Vec<BattingOrderHistory>,
}
impl GameResult {
    pub fn new(
        id: u32,
        actual_date: NaiveDate,
        away_team_id: u16,
        home_team_id: u16,
        away_batting_orders: Vec<BattingOrder>,
        home_batting_orders: Vec<BattingOrder>,
    ) -> Self {
        let batting_order_histories = Self::init_batting_order_histories(
            away_team_id,
            home_team_id,
            away_batting_orders,
            home_batting_orders,
        );
        Self {
            id: id,
            actual_date: actual_date,
            innings: Vec::new(),
            away_points: 0,
            home_points: 0,
            batting_order_histories: batting_order_histories,
        }
    }
    pub fn update_point(&mut self, game_state: &GameState) {
        if game_state.inning_tb == TB::Bottom {
            self.home_points = game_state.home_total_point;
        } else {
            self.away_points = game_state.away_total_point;
        }
    }

    fn init_batting_order_histories(
        away_team_id: u16,
        home_team_id: u16,
        away_team_batting_orders: Vec<BattingOrder>,
        home_team_batting_orders: Vec<BattingOrder>,
    ) -> Vec<BattingOrderHistory> {
        let mut batting_order_histories = Vec::new();

        for away_team_batting_order in away_team_batting_orders {
            batting_order_histories.push(Self::add_batting_order_hitstory(
                1,
                TB::Top,
                1,
                None,
                None,
                None,
                away_team_id,
                away_team_batting_order,
            ));
        }
        for home_team_batting_order in home_team_batting_orders {
            batting_order_histories.push(Self::add_batting_order_hitstory(
                1,
                TB::Bottom,
                1,
                None,
                None,
                None,
                home_team_id,
                home_team_batting_order,
            ));
        }

        batting_order_histories
    }

    fn add_batting_order_hitstory(
        start_inning_seq: u8,
        start_inning_tb: TB,
        start_count_seq: u8,
        end_inning_seq: Option<u8>,
        end_inning_tb: Option<TB>,
        end_count_seq: Option<u8>,
        team_id: u16,
        batting_order: BattingOrder,
    ) -> BattingOrderHistory {
        BattingOrderHistory::new(
            start_inning_seq,
            start_inning_tb,
            start_count_seq,
            end_inning_seq,
            end_inning_tb,
            end_count_seq,
            team_id,
            batting_order.index,
            batting_order.position,
            batting_order.player,
        )
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
    pub batting_order_histories: Vec<BattingOrderHistory>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
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

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct Inning {
    pub seq: u8,
    pub tb: TB,
    pub counts: Vec<Count>,
}
impl Inning {
    pub fn is(&self, seq: u8, tb: TB) -> bool {
        self.seq == seq && self.tb == tb
    }

    pub fn add_count(&mut self, count: Count) {
        self.counts.push(count);
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct Count {
    pub seq: u8,
    pub bases_occupied: u8,
    pub result: BattingResult,
    pub point: u8,
    pub ball: u8,
    pub strike: u8,
    pub out: u8,
}

#[derive(Clone, PartialEq, Eq, EnumString, Serialize, Deserialize, Debug, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum BattingResult {
    Single,
    Double,
    Triple,
    HomeRun,
    Out,
}
impl std::fmt::Display for BattingResult {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BattingResult::Single => write!(f, "{}", t!("single")),
            BattingResult::Double => write!(f, "{}", t!("double")),
            BattingResult::Triple => write!(f, "{}", t!("triple")),
            BattingResult::HomeRun => write!(f, "{}", t!("homerun")),
            BattingResult::Out => write!(f, "{}", t!("out")),
        }
    }
}
impl BattingResult {
    pub fn is_out(&self) -> bool {
        matches!(self, BattingResult::Out)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Base {
    First = 0,
    Second = 1,
    Third = 2,
}
