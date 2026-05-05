use crate::t;
use serde::{Deserialize, Serialize};
use strum_macros::EnumString;

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
