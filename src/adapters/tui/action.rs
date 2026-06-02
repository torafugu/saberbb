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
            Self::ViewStandings => t!("view_standings"),
            Self::ViewGameResults => t!("view_game_results"),
            Self::ViewBattingStat => t!("view_batting_stat"),
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Display, Serialize, Deserialize)]
pub enum Action {
    Tick,
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
    MenuItemSelected(MenuOption),
}
