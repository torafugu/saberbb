use crate::domain::shared::types::Position;
use crate::domain::utils;
use crate::t;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use strum_macros::EnumString;

const BATTING_MIN_HIT_AVERAGE: f64 = 0.2;
const BATTING_MAX_HIT_AVERAGE: f64 = 0.32;
const BATTING_MIN_SLG: f64 = 0.3;
const BATTING_MAX_SLG: f64 = 0.55;
const PLAYER_NAME_DEFAULT: &str = "DEAFULT";
const PLAYER_AGE_DEFAULT: u8 = 25;

#[derive(Clone, Serialize, Deserialize, EnumString, Debug)]
#[strum(ascii_case_insensitive)]
pub enum RL {
    Right,
    Left,
}
impl fmt::Display for RL {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RL::Right => write!(f, "{}", t!("right")),
            RL::Left => write!(f, "{}", t!("left")),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Player {
    pub id: u32, // in case of same first_name and last_name
    pub first_name: Arc<str>,
    pub last_name: Arc<str>,
    pub age: u8,
    pub throw: RL,
    pub mod_speed: f64,
    pub mod_control: f64,
    pub defensive_skills: Vec<DefensiveSkill>,
    pub bat: RL,
    pub mod_ba: f64,
    pub mod_slg: f64,
}

impl Player {
    pub fn default() -> Self {
        Player::min(1, PLAYER_NAME_DEFAULT, PLAYER_NAME_DEFAULT)
    }

    pub fn min(id: u32, first_name: &str, last_name: &str) -> Self {
        Self {
            id: id,
            first_name: Arc::from(first_name),
            last_name: Arc::from(last_name),
            age: PLAYER_AGE_DEFAULT,
            throw: RL::Right,
            mod_speed: 0.0,
            mod_control: 0.0,
            defensive_skills: Vec::new(),
            bat: RL::Right,
            mod_ba: 0.0,
            mod_slg: 0.0,
        }
    }

    pub fn batter(id: u32, first_name: &str, last_name: &str, mod_ba: f64, mod_slg: f64) -> Self {
        Self {
            id: id,
            first_name: Arc::from(first_name),
            last_name: Arc::from(last_name),
            age: 25,
            throw: RL::Right,
            mod_speed: 0.0,
            mod_control: 0.0,
            defensive_skills: Vec::new(),
            bat: RL::Right,
            mod_ba: mod_ba,
            mod_slg: mod_slg,
        }
    }

    pub fn hit_average(&self) -> f64 {
        let ba = (BATTING_MAX_HIT_AVERAGE + BATTING_MIN_HIT_AVERAGE) * 0.5
            + (BATTING_MAX_HIT_AVERAGE - BATTING_MIN_HIT_AVERAGE)
                * (utils::sigmoid(self.mod_ba) - 0.5);
        ba
    }

    pub fn slg(&self) -> f64 {
        let slg = (BATTING_MAX_SLG + BATTING_MIN_SLG) * 0.5
            + (BATTING_MAX_SLG - BATTING_MIN_SLG) * (utils::sigmoid(self.mod_slg) - 0.5);
        slg
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DefensiveSkill {
    position: Position,
    mod_uzr: f64,
}
