use serde::{Deserialize, Serialize};
use std::fmt;
use strum::{Display, EnumIter};

use crate::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Serialize, Deserialize)]
pub enum MenuOption {
    ViewStandings,
    ViewGameResults,
    ViewBattingStat,
}

impl fmt::Display for MenuOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::ViewStandings => t!("standings"),
            Self::ViewGameResults => t!("game_results"),
            Self::ViewBattingStat => t!("batting_stats"),
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Display, Serialize, Deserialize)]
pub enum Action {
    Render,
    Resize(u16, u16),
    Suspend,
    Resume,
    Quit,
    ClearScreen,
    Error(String),
    Help,
    SelectNext,
    SelectPrevious,
    ConfirmSelection,
    SelectGameDetailTab(usize),
    NextCount,
    PreviousCount,
    MenuItemSelected(MenuOption),
}
