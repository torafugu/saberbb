mod adapters;
mod domains;
mod game_engine;
mod i18n;
mod repositories;
mod resolver;
mod scheduler;

use adapters::game_presenter::display_game_rounds_processed;
use adapters::schedule_presenter::display_game_seasons_scheduled;
use adapters::topmenu_presenter::display_menu;
use clap::Parser;
use game_engine::process_game;
use i18n::I18nManager;
use scheduler::schedule_season;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct AppConfig {
    version: u8,
    language: String,
}
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            language: String::from("en-US"),
        }
    }
}

fn main() {
    // load default-config.toml and initialize I18nManager
    let cfg: AppConfig = confy::load::<AppConfig>("statbb", None).unwrap_or_default();
    I18nManager::init(&cfg.language);

    let args = Args::parse();

    // Game Process Mode
    if let Some(num_of_rounds) = args.process {
        for _ in 0..num_of_rounds {
            if let Err(e) = process_game() {
                eprintln!("{}:{}", t!("error", "function" => "process_game"), e);
            }
        }
        display_game_rounds_processed(num_of_rounds);
    }

    // Game Schedule Generate Mode
    if let Some(num_of_schedules) = args.schedule {
        for _ in 0..num_of_schedules {
            if let Err(e) = schedule_season() {
                eprintln!("{}:{}", t!("error", "function" => "schedule_season"), e);
            }
        }
        display_game_seasons_scheduled(num_of_schedules);
    }

    // Game Display Mode　(Show the game result interactively)
    if args.menu {
        display_menu();
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Proess games
    #[arg(short, long)]
    process: Option<i8>,

    /// Display the last game result
    #[arg(short, long)]
    display: bool,

    /// View game result interactively
    #[arg(short, long)]
    menu: bool,

    /// Schedule games
    #[arg(short, long)]
    schedule: Option<i8>,
}
