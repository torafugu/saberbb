use super::player::Batter;

use std::fmt;
use std::sync::Arc;

const SINGLE_TEXT: &str = "Single Hit!";
const DOUBLE_TEXT: &str = "Double!";
const TRIPLE_TEXT: &str = "Triple!";
const HOME_RUN_TEXT: &str = "Home Run!";
const OUT_TEXT: &str = "Out!";

#[derive(Copy, Clone)]
pub enum InningType {
    Top,
    Bottom,
}

#[derive(Clone)]
pub struct Inning {
    //pub tb: InningType,
    //pub seq: i8,
    pub counts: Vec<Count>,
    pub score: i8,
}

#[derive(Clone)]
pub struct Count {
    pub seq: i32,
    pub is_first_runner: bool,
    pub is_second_runner: bool,
    pub is_third_runner: bool,
    pub batter: Arc<Batter>,
    pub result: BattingResult,
    pub score: i8,
    pub out: i8,
}

#[derive(Clone, PartialEq, Eq)]
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
