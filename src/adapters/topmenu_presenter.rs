use super::game_presenter::{display_select_season, display_standings};
use crate::t;
use console::Term;
use inquire::Select;
use std::fmt;
use strum::{EnumIter, IntoEnumIterator};

#[derive(Debug, PartialEq, EnumIter)]
pub enum MenuOption {
    ViewStandings,
    ViewGameResults,
    Exit,
}
impl fmt::Display for MenuOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::ViewStandings => t!("view_standings"),
            Self::ViewGameResults => t!("view_game_results"),
            Self::Exit => t!("exit"),
        };
        write!(f, "{}", label)
    }
}

pub fn display_menu() {
    let term = Term::stdout();
    term.clear_last_lines(100).unwrap();

    let options: Vec<MenuOption> = MenuOption::iter().collect();

    let selection = Select::new(&t!("main_menu"), options)
        .with_help_message(&t!("help_message"))
        .prompt();

    match selection {
        Ok(MenuOption::ViewStandings) => {
            term.clear_screen().unwrap();
            // println!("Selected: {}", selection);
            display_standings();
        }
        Ok(MenuOption::ViewGameResults) => {
            term.clear_screen().unwrap();
            // println!("Selected: {}", selection);
            display_select_season();
        }
        Ok(MenuOption::Exit) => std::process::exit(0),
        Err(_) => std::process::exit(0),
    }

    // loop {}
}
