use serde::{Deserialize, Serialize};
use std::fmt;
use strum::{Display, EnumIter};


#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Serialize, Deserialize)]
pub enum MenuOption {
    Schedule,
    Generate,
    Process,
    Analyze,
}

impl fmt::Display for MenuOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Schedule => "Schedule seasons / 日程作成",
            Self::Generate => "Generate players & managers / キャラクター作成",
            Self::Process => "Process rounds / 試合進行",
            Self::Analyze => "Analysis rounds / 診断付き試合進行",
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
    Back,
    SelectNext,
    SelectPrevious,
    ConfirmSelection,
    NextCount,
    PreviousCount,
    MenuItemSelected(MenuOption),
    ManagementFinished(String),
}
