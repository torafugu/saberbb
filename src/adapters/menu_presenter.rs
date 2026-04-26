use super::game_presenter::display_select_season;
use crate::t;
use console::Term;
use dialoguer::theme::ColorfulTheme;
use inquire::Select;
use std::fmt;
use strum::{EnumIter, IntoEnumIterator};
// use inquire::ui::{Attributes, RenderConfig, StyleSheet};
// use inquire::{Select, Text};

#[derive(Debug, PartialEq, EnumIter)]
pub enum MenuOption {
    ViewResultThisSeason,
    ViewResultPastSeasons,
    Exit,
}
impl fmt::Display for MenuOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::ViewResultThisSeason => t!("view_game_result_this_season"),
            Self::ViewResultPastSeasons => t!("view_game_result_past_seasons"),
            Self::Exit => t!("exit"),
        };
        write!(f, "{}", label)
    }
}
// impl MenuOption {
//     pub fn label(&self) -> String {
//         match self {
//             Self::ViewResultThisSeason => t!("view_game_result_this_season"),
//             Self::ViewResultPastSeasons => t!("view_game_result_past_seasons"),
//             Self::Exit => t!("exit"),
//         }
//     }
// }

pub fn display_menu() {
    let term = Term::stdout();
    term.clear_last_lines(100).unwrap();

    let options: Vec<MenuOption> = MenuOption::iter().collect();

    let selection = Select::new(&t!("main_menu"), options)
        .with_help_message(&t!("help_message"))
        .prompt();

    match selection {
        Ok(MenuOption::ViewResultThisSeason) => {
            term.clear_screen().unwrap();
            // println!("Selected: {}", selection);
            display_select_season();
        }
        Ok(MenuOption::ViewResultPastSeasons) => {
            term.clear_screen().unwrap();
            // println!("Selected: {}", selection);
            display_select_season();
        }
        Ok(MenuOption::Exit) => std::process::exit(0),
        Err(_) => std::process::exit(0),
    }

    // loop {}
}
