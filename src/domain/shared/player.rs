use crate::I18nManager;
use crate::domain::util;
use crate::t;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use strum_macros::{AsRefStr, EnumString};
use validator::Validate;

const BATTING_MIN_HIT_AVERAGE: f64 = 0.2;
const BATTING_MAX_HIT_AVERAGE: f64 = 0.32;
const BATTING_MIN_SLG: f64 = 0.3;
const BATTING_MAX_SLG: f64 = 0.55;

#[derive(Clone, Hash, PartialEq, Eq, EnumString, Serialize, Deserialize, Debug, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum Position {
    P,
    C,
    FB,
    SB,
    TB,
    SS,
    LF,
    CF,
    RF,
    DH,
}
impl Position {
    pub const ALL: [Position; 10] = [
        Position::P,
        Position::C,
        Position::FB,
        Position::SB,
        Position::TB,
        Position::SS,
        Position::LF,
        Position::CF,
        Position::RF,
        Position::DH,
    ];
    pub const ALL_NO_DH: [Position; 9] = [
        Position::P,
        Position::C,
        Position::FB,
        Position::SB,
        Position::TB,
        Position::SS,
        Position::LF,
        Position::CF,
        Position::RF,
    ];
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Position::P => write!(f, "{}", t!("p")),
            Position::C => write!(f, "{}", t!("c")),
            Position::FB => write!(f, "{}", t!("fb")),
            Position::SB => write!(f, "{}", t!("sb")),
            Position::TB => write!(f, "{}", t!("tb")),
            Position::SS => write!(f, "{}", t!("ss")),
            Position::LF => write!(f, "{}", t!("lf")),
            Position::CF => write!(f, "{}", t!("cf")),
            Position::RF => write!(f, "{}", t!("rf")),
            Position::DH => write!(f, "{}", t!("dh")),
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize, EnumString, Debug, AsRefStr)]
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

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct FullName {
    pub first: Arc<str>,
    pub last: Arc<str>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct Player {
    pub id: u32, // in case of same first_name and last_name
    pub first_name: Arc<str>,
    pub last_name: Arc<str>,
    pub age: u8,
    pub throw: RL,
    pub defensive_skills: Vec<DefensiveSkill>,
    pub pitcher_attribute: Option<PitcherAttribute>,
    pub bat: RL,
    pub mod_ba: f64,
    pub mod_slg: f64,
}

impl Player {
    pub fn new_unsaved(
        first_name: &str,
        last_name: &str,
        age: u8,
        throw: RL,
        defensive_skills: Vec<DefensiveSkill>,
        pitcher_attribute: Option<PitcherAttribute>,
        bat: RL,
        mod_ba: f64,
        mod_slg: f64,
    ) -> Self {
        Self {
            id: 0,
            first_name: Arc::from(first_name),
            last_name: Arc::from(last_name),
            age: age,
            throw: throw,
            defensive_skills: defensive_skills,
            pitcher_attribute: pitcher_attribute,
            bat: bat,
            mod_ba: mod_ba,
            mod_slg: mod_slg,
        }
    }

    pub fn new(
        id: u32,
        first_name: &str,
        last_name: &str,
        age: u8,
        throw: RL,
        bat: RL,
        mod_ba: f64,
        mod_slg: f64,
    ) -> Self {
        Self {
            id: id,
            first_name: Arc::from(first_name),
            last_name: Arc::from(last_name),
            age: age,
            throw: throw,
            defensive_skills: Vec::new(),
            pitcher_attribute: None,
            bat: bat,
            mod_ba: mod_ba,
            mod_slg: mod_slg,
        }
    }

    pub fn full_name(&self) -> String {
        I18nManager::global().full_name(&self.first_name, &self.last_name)
    }

    pub fn hit_average(&self) -> f64 {
        let ba = (BATTING_MAX_HIT_AVERAGE + BATTING_MIN_HIT_AVERAGE) * 0.5
            + (BATTING_MAX_HIT_AVERAGE - BATTING_MIN_HIT_AVERAGE)
                * (util::sigmoid(self.mod_ba) - 0.5);
        ba
    }

    pub fn slg(&self) -> f64 {
        let slg = (BATTING_MAX_SLG + BATTING_MIN_SLG) * 0.5
            + (BATTING_MAX_SLG - BATTING_MIN_SLG) * (util::sigmoid(self.mod_slg) - 0.5);
        slg
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct DefensiveSkill {
    pub position: Position,
    pub mod_uzr: f64,
}

#[derive(Clone, Serialize, Deserialize, EnumString, Debug, AsRefStr)]
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

#[derive(Clone, Serialize, Deserialize, EnumString, Debug, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum PitcherStyle {
    PowerPitcher,
    FinessePitcher,
    BalancedPitcher,
}
impl fmt::Display for PitcherStyle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            PitcherStyle::PowerPitcher => write!(f, "{}", t!("power_pitcher")),
            PitcherStyle::FinessePitcher => write!(f, "{}", t!("finesse_pitcher")),
            PitcherStyle::BalancedPitcher => write!(f, "{}", t!("balanced_pitcher")),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PitcherAttribute {
    pub pitcher_style: PitcherStyle,
    pub mod_velocity: f64,
    pub mod_control: f64,
    pub mod_stamina: f64,
    pub mod_injury_proneness: f64,
    pub mod_clutch: f64,
    pub mod_hpp: f64, // Home-Away Splitting
    pub mod_platoon_splitting: f64,
    pub pitch_skills: Vec<PitchSkill>,
}
impl PitcherAttribute {
    pub fn from_prob(
        pitcher_style: PitcherStyle,
        mod_velocity: f64,
        mod_control: f64,
        mod_stamina: f64,
        mod_injury_proneness: f64,
        mod_clutch: f64,
        mod_hpp: f64,
        mod_platoon_splitting: f64,
        pitch_skills: Vec<PitchSkill>,
    ) -> Self {
        Self {
            pitcher_style: pitcher_style,
            mod_velocity: mod_velocity,
            mod_control: mod_control,
            mod_stamina: mod_stamina,
            mod_injury_proneness: mod_injury_proneness,
            mod_clutch: mod_clutch,
            mod_hpp: mod_hpp,
            mod_platoon_splitting: mod_platoon_splitting,
            pitch_skills: pitch_skills,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
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

impl PitchSkill {
    pub fn from_prob(
        pitch_type: PitchType,
        mod_velocity: f64,
        mod_control: f64,
        mod_stamina: f64,
        mod_injury_proneness: f64,
        mod_stuff: f64,
        mod_fb: f64,
        mod_gp: f64,
        mod_horizontal_movement: f64,
        mod_vertical_movement: f64,
        mod_spin_rate: f64,
        mod_usage: f64,
    ) -> Self {
        Self {
            pitch_type: pitch_type,
            mod_velocity: mod_velocity,
            mod_control: mod_control,
            mod_stamina: mod_stamina,
            mod_injury_proneness: mod_injury_proneness,
            mod_stuff: mod_stuff,
            mod_fb: mod_fb,
            mod_gp: mod_gp,
            mod_horizontal_movement: mod_horizontal_movement,
            mod_vertical_movement: mod_vertical_movement,
            mod_spin_rate: mod_spin_rate,
            mod_usage: mod_usage,
        }
    }
}
