mod adapters;
mod domain;
mod i18n;
mod repositories;

use adapters::game_presenter::display_game_rounds_processed;
use adapters::schedule_presenter::display_game_seasons_scheduled;
use adapters::topmenu_presenter::display_menu;
use clap::Parser;
use deadpool::managed::Pool;
use domain::game_service::GameService;
use domain::player_service::PlayerService;
use domain::schedule_service::ScheduleService;
use i18n::I18nManager;
use repositories::game_repository::SqlGameRepository;
use repositories::persistence_config::get_sqlite_manager;
use repositories::player_repository::SqlPlayerRepository;
use repositories::schedule_repository::SqlScheduleRepository;
use repositories::statistics_repository::SqlStatRepository;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

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

struct AppContext {
    game_repository: SqlGameRepository,
    player_repository: SqlPlayerRepository,
    schedule_repository: SqlScheduleRepository,
    statistics_repository: SqlStatRepository,
}

static APP_CONTEXT: OnceLock<AppContext> = OnceLock::new();

// fn ctx() -> &'static AppContext {
//     APP_CONTEXT
//         .get()
//         .expect(&t!("not_initialized", "struct" => "AppContext"))
// }

fn main() {
    // load default-config.toml and initialize I18nManager
    let cfg: AppConfig = confy::load::<AppConfig>("statbb", None).unwrap_or_default();
    I18nManager::init(&cfg.language);

    // TODO: move to separated function
    let manager = get_sqlite_manager().expect(&t!("dbpool_failed"));
    let pool = Pool::builder(manager).max_size(16).build().unwrap();

    let ctx = AppContext {
        game_repository: SqlGameRepository { pool: pool.clone() },
        player_repository: SqlPlayerRepository { pool: pool.clone() },
        schedule_repository: SqlScheduleRepository { pool: pool.clone() },
        statistics_repository: SqlStatRepository { pool: pool.clone() },
    };
    APP_CONTEXT.set(ctx).ok();

    let args = Args::parse();

    // Game Process Mode
    if let Some(num_of_rounds) = args.process {
        let mut game_service = GameService {
            repo: APP_CONTEXT.get().unwrap().game_repository.clone(),
        };

        for _ in 0..num_of_rounds {
            if let Err(e) = game_service.process_game_round() {
                eprintln!("{}:{}", t!("error", "function" => "process_game"), e);
            }
        }
        display_game_rounds_processed(num_of_rounds);
    }

    // Player Generate Mode
    if let Some(num_of_players) = args.generate {
        let mut player_service = PlayerService {
            repo: APP_CONTEXT.get().unwrap().player_repository.clone(),
        };
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
        let mut schedule_service = ScheduleService {
            repo: APP_CONTEXT.get().unwrap().schedule_repository.clone(),
        };
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
