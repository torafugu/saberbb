use crate::t;
use serde::{Deserialize, Serialize};
use strum_macros::EnumString;

#[derive(Clone, PartialEq, Eq, EnumString, Serialize, Deserialize, Debug)]
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

#[derive(Copy, Clone, PartialEq, Eq, Hash, EnumString, Serialize, Deserialize, Debug)]
#[strum(ascii_case_insensitive)]
pub enum InningType {
    Top,
    Bottom,
}
impl std::fmt::Display for InningType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            InningType::Top => write!(f, "{}", t!("inning_top")),
            InningType::Bottom => write!(f, "{}", t!("inning_bottom")),
        }
    }
}

#[derive(Clone, PartialEq, Eq, EnumString, Serialize, Deserialize, Debug)]
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
