use anyhow::{Result, bail};
use clap::Parser;
use saberbb::adapters::cui::game_cui::display_game_rounds_processed;
use saberbb::adapters::cui::schedule_cui::display_game_seasons_scheduled;
use saberbb::adapters::cui::top_cui::display_menu;
use saberbb::adapters::tui::top_tui::menu;
use saberbb::config::load_app_config;
use saberbb::domain::game_service::GameService;
use saberbb::domain::player_factory::PlayerFactory;
use saberbb::domain::player_service::PlayerService;
use saberbb::domain::schedule_service::ScheduleService;
use saberbb::i18n::I18nManager;
use saberbb::repositories::game_repository::SqlGameRepository;
use saberbb::repositories::player_repository::SqlPlayerRepository;
use saberbb::repositories::schedule_repository::SqlScheduleRepository;
use saberbb::repositories::statistics_repository::SqlStatRepository;
use saberbb::{AppContext, app_context, init_app_context, t};
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    // load default-config.toml and initialize I18nManager
    let cfg = load_app_config()?;

    I18nManager::init(&cfg.language);

    let ctx = AppContext {
        game_repository: SqlGameRepository::new()?,
        player_repository: SqlPlayerRepository::new()?,
        schedule_repository: SqlScheduleRepository::new()?,
        statistics_repository: SqlStatRepository::new()?,
    };
    init_app_context(ctx);

    let file_appender = tracing_appender::rolling::daily("log", "app.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    // Game Process Mode
    if let Some(num_of_games) = args.process {
        let mut game_service = GameService {
            repo: app_context().game_repository.clone(),
        };

        for _ in 0..num_of_games {
            if let Err(e) = game_service.process_game_round() {
                let error_msg = t!("error", "function" => "process_game");
                bail!("{}, {}", error_msg, e);
            }
            info!("1 game processed.");
        }
        display_game_rounds_processed(num_of_games);
    }

    // Player Generate Mode
    if let Some(num_of_players) = args.generate {
        let player_service = PlayerService {
            repo: app_context().player_repository.clone(),
        };
        let mut player_factory = PlayerFactory::new(player_service);
        if let Err(e) = player_factory.generate_and_save_players(num_of_players) {
            let error_msg = t!("error", "function" => "generate_players");
            bail!("{}, {}", error_msg, e);
        }
    }

    // Game Display Mode　(Show the game result interactively)
    if args.menu {
        display_menu();
    }

    // Game Schedule Generate Mode
    if let Some(num_of_schedules) = args.schedule {
        let mut schedule_service = ScheduleService {
            repo: app_context().schedule_repository.clone(),
        };
        for _ in 0..num_of_schedules {
            if let Err(e) = schedule_service.schedule_season() {
                let error_msg = t!("error", "function" => "schedule_season");
                bail!("{}, {}", error_msg, e);
            }
            info!("1 season scheduled.");
        }
        display_game_seasons_scheduled(num_of_schedules);
    }

    if args.top {
        let _ = menu(cfg.clone());
    }

    Ok(())
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
    generate: Option<u16>,

    /// View game result interactively
    #[arg(short, long)]
    menu: bool,

    /// TUI game menu
    #[arg(short, long)]
    top: bool,

    /// Schedule games
    #[arg(short, long)]
    schedule: Option<u8>,
}
