mod adapters;
mod domain;
mod i18n;
mod repositories;

use adapters::game_presenter::display_game_rounds_processed;
use adapters::schedule_presenter::display_game_seasons_scheduled;
use adapters::topmenu_presenter::display_menu;
use clap::Parser;
use domain::game_service::GameService;
use domain::player_service::PlayerService;
use domain::schedule_service::ScheduleService;
use i18n::I18nManager;
use repositories::game_repository::SqlGameRepository;
use repositories::persistence_config::get_db_conn;
use repositories::player_repository::SqlPlayerRepository;
use repositories::schedule_repository::SqlScheduleRepository;
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
        let db_repo = SqlGameRepository {
            pool: get_db_conn().unwrap(),
        };
        let mut game_service = GameService { repo: db_repo };

        for _ in 0..num_of_rounds {
            if let Err(e) = game_service.process_game_round() {
                eprintln!("{}:{}", t!("error", "function" => "process_game"), e);
            }
        }
        display_game_rounds_processed(num_of_rounds);
    }

    // Player Generate Mode
    if let Some(num_of_players) = args.generate {
        let db_repo = SqlPlayerRepository {
            pool: get_db_conn().unwrap(),
        };
        let mut player_service = PlayerService { repo: db_repo };
        if let Err(e) = player_service.generate_players(num_of_players) {
            eprintln!("{}:{}", t!("error", "function" => "generate_players"), e);
        }
    }

    // Game Display Mode　(Show the game result interactively)
    if args.menu {
        display_menu();
    }

    // Game Schedule Generate Mode
    if let Some(num_of_schedules) = args.schedule {
        let db_repo = SqlScheduleRepository {
            pool: get_db_conn().unwrap(),
        };
        let mut schedule_service = ScheduleService { repo: db_repo };
        for _ in 0..num_of_schedules {
            if let Err(e) = schedule_service.schedule_season() {
                eprintln!("{}:{}", t!("error", "function" => "schedule_season"), e);
            }
        }
        display_game_seasons_scheduled(num_of_schedules);
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Process games
    #[arg(short, long)]
    process: Option<i8>,

    /// Display the last game result
    #[arg(short, long)]
    display: bool,

    /// Generate players
    #[arg(short, long)]
    generate: Option<i16>,

    /// View game result interactively
    #[arg(short, long)]
    menu: bool,

    /// Schedule games
    #[arg(short, long)]
    schedule: Option<i8>,
}
