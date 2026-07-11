use anyhow::{Result, bail};
use clap::Parser;
use saberbb::adapters::tui::top_tui::menu;
use saberbb::config::load_app_config;
use saberbb::domain::game_service::GameService;
use saberbb::domain::player_factory::PlayerFactory;
use saberbb::domain::player_service::PlayerService;
use saberbb::domain::schedule_service::ScheduleService;
use saberbb::i18n::I18nManager;
use saberbb::repositories::db::maintenance;
use saberbb::repositories::game_repository::SqlGameRepository;
use saberbb::repositories::player_repository::SqlPlayerRepository;
use saberbb::repositories::schedule_repository::SqlScheduleRepository;
use saberbb::repositories::statistics_repository::SqlStatRepository;
use saberbb::{AppContext, app_context, init_app_context, t};
use saberbb::{init_dirs, proj_dirs};
use std::backtrace::Backtrace;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    // load default-config.toml and initialize I18nManager
    let cfg = load_app_config()?;
    I18nManager::init(&cfg.language);

    init_dirs()?;
    let log_dir = proj_dirs().data_dir().join("log");

    let file_appender = tracing_appender::rolling::daily(log_dir, "app.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339())
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(non_blocking)
        .init();

    std::panic::set_hook(Box::new(|panic_info| {
        let backtrace = Backtrace::force_capture();
        tracing::error!(%panic_info, %backtrace, "panic occurred");
    }));

    let ctx = AppContext {
        game_repository: SqlGameRepository::new()?,
        player_repository: SqlPlayerRepository::new()?,
        schedule_repository: SqlScheduleRepository::new()?,
        statistics_repository: SqlStatRepository::new()?,
    };
    init_app_context(ctx);

    let args = Args::parse();

    info!("game started.");

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
        println!(
            "{}",
            t!("game_rounds_processed", "num_of_rounds" => num_of_games.to_string())
        );
    }

    // Player Generate Mode
    if let Some(num_of_players) = args.generate {
        let player_service = PlayerService {
            repo: app_context().player_repository.clone(),
        };
        let mut player_factory = PlayerFactory::new(player_service);
        player_factory.load_player_probs()?;

        for _ in 0..num_of_players {
            if let Err(e) = player_factory.generate_and_save_player() {
                let error_msg = t!("error", "function" => "generate_players");
                bail!("{}, {}", error_msg, e);
            }
            info!("1 player generated.");
            println!("{}", t!("player_generated"));
        }
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
        println!(
            "{}",
            t!("game_seasons_scheduled", "num_of_seasons" => num_of_schedules.to_string())
        );
    }

    // Execute VACUUM and PRAGMA optimize
    if args.maintenance {
        maintenance()?;
    }

    // TUI mode
    if args.view {
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

    /// Generate players
    #[arg(short, long)]
    generate: Option<u16>,

    /// DB maintenance
    #[arg(short, long)]
    maintenance: bool,

    /// Schedule games
    #[arg(short, long)]
    schedule: Option<u8>,

    /// Show TUI game menu
    #[arg(short, long)]
    view: bool,
}
