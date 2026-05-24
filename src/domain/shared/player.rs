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

#[derive(Clone, PartialEq, Serialize, Deserialize, EnumString, Debug)]
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
    pub defensive_skills: Vec<DefensiveSkill>,
    pub pitcher_skill: Option<PitcherSkill>,
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
            pitcher_skill: None,
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
            defensive_skills: Vec::new(),
            pitcher_skill: None,
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
pub struct PlayerAttributeProb {
    pub age_shape: f64,
    pub age_scale: f64,
    pub age_offset: f64,
    pub throw_lefty: f64,
    pub bat_lefty: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct BatterSkillProb {
    pub ba_skew: f64,
    pub slg_skew: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DefensiveSkill {
    pub position: Position,
    pub mod_uzr: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DefensiveSkillProb {
    pub uzr_skew: f64,
}

#[derive(Clone, Serialize, Deserialize, EnumString, Debug)]
#[strum(ascii_case_insensitive)]
pub enum PitchType {
    FourSeamFastball,
    TwoSeamFastball,
    Cutter,
    Curveball,
    Slider,
    Sweeper,
    Changeup,
    Forkball,
    SplitFingerFastball,
    Knuckleball,
}
impl fmt::Display for PitchType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            PitchType::FourSeamFastball => write!(f, "{}", t!("four_seam_fastball")),
            PitchType::TwoSeamFastball => write!(f, "{}", t!("two_seam_fastball")),
            PitchType::Cutter => write!(f, "{}", t!("cutter")),
            PitchType::Curveball => write!(f, "{}", t!("curveball")),
            PitchType::Slider => write!(f, "{}", t!("slider")),
            PitchType::Sweeper => write!(f, "{}", t!("sweeper")),
            PitchType::Changeup => write!(f, "{}", t!("changeup")),
            PitchType::Forkball => write!(f, "{}", t!("forkball")),
            PitchType::SplitFingerFastball => write!(f, "{}", t!("split_finger_fastball")),
            PitchType::Knuckleball => write!(f, "{}", t!("knuckleball")),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PitcherSkill {
    pub mod_velocity: f64,
    pub mod_control: f64,
    pub mod_stamina: f64,
    pub mod_injury_proneness: f64,
    pub mod_clutch: f64,
    pub mod_hpp: f64, // Home-Away Splitting
    pub mod_platoon_splitting: f64,
    pub pitch_skills: Vec<PitchSkill>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PitcherBaseSkillProb {
    pub velocity_skew: f64,
    pub control_skew: f64,
    pub stamina_skew: f64,
    pub injury_proneness_skew: f64,
    pub clutch_skew: f64,
    pub hpp_skew: f64,
    pub platoon_splitting_skew: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PitchSkill {
    pub pitch_type: PitchType,
    pub mod_velocity: f64,
    pub mod_control: f64,
    pub mod_stamina: f64,
    pub mod_injury_proneness: f64,
    pub mod_stuff: f64,
    pub mod_fb: f64, // Home Run to Fly Ball Rate
    pub mod_gp: f64, // Grounder Percentage
    pub mod_horizontal_movement: f64,
    pub mod_vertical_movement: f64,
    pub mod_spin_rate: f64,
    pub mod_usage: f64, // TODO: Should be over written by strategy
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PitchSkillProb {
    pub pitch_type: PitchType,
    pub velocity_skew: f64,
    pub control_skew: f64,
    pub stamina_skew: f64,
    pub injury_proneness_skew: f64,
    pub stuff_skew: f64,
    pub fb_skew: f64,
    pub gp_skew: f64,
    pub horizontal_movement_skew: f64,
    pub vertical_movement_skew: f64,
    pub spin_rate_skew: f64,
    pub usage_skew: f64,
}
