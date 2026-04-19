use std::fmt;
use strum::EnumString;

const SINGLE_TEXT: &str = "Single";
const DOUBLE_TEXT: &str = "Double";
const TRIPLE_TEXT: &str = "Triple";
const HOME_RUN_TEXT: &str = "HomeRun";
const OUT_TEXT: &str = "Out";
const INNING_TOP_TEXT: &str = "Top";
const INNING_BOTTOM_TEXT: &str = "Bottom";

#[derive(Copy, Clone, PartialEq, Eq, Hash, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum InningType {
    TOP,
    BOTTOM,
}
impl fmt::Display for InningType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            InningType::TOP => write!(f, "{INNING_TOP_TEXT}"),
            InningType::BOTTOM => write!(f, "{INNING_BOTTOM_TEXT}"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum BattingResult {
    SINGLE,
    DOUBLE,
    TRIPLE,
    HOMERUN,
    OUT,
}
impl fmt::Display for BattingResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            BattingResult::SINGLE => write!(f, "{SINGLE_TEXT}"),
            BattingResult::DOUBLE => write!(f, "{DOUBLE_TEXT}"),
            BattingResult::TRIPLE => write!(f, "{TRIPLE_TEXT}"),
            BattingResult::HOMERUN => write!(f, "{HOME_RUN_TEXT}"),
            BattingResult::OUT => write!(f, "{OUT_TEXT}"),
        }
    }
}
