use anyhow::{Result, bail};
use chrono::{Datelike, Local};
use clap::Parser;
use saberbb::adapters::tui::top_tui::menu;
use saberbb::config::load_app_config;
use saberbb::domain::game_service::GameService;
use saberbb::domain::player_factory::PlayerFactory;
use saberbb::domain::player_service::PlayerService;
use saberbb::domain::schedule_service::ScheduleService;
use saberbb::i18n::I18nManager;
use saberbb::repositories::db::{SqlDb, maintenance};
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

    if args.newyear {
        newyear()?;
    }

    // Game Process Mode
    if let Some(num_of_games) = args.process {
        process_games(num_of_games)?;
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
        schedule_games(num_of_schedules)?;
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

fn newyear() -> Result<()> {
    let mut conn = SqlDb::new()?.get_conn()?;
    reset_season(&mut conn, Local::now().year())?;
    schedule_games(1)?;
    process_games(40)?;
    Ok(())
}

fn reset_season(conn: &mut rusqlite::Connection, year: i32) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute_batch(
        "DELETE FROM count;
         DELETE FROM inning;
         DELETE FROM game;
         DELETE FROM player_game_batting;
         DELETE FROM player_game_entry;
         DELETE FROM player_game_fielding;
         DELETE FROM player_game_running;
         DELETE FROM player_game_pitching;",
    )?;
    tx.execute("UPDATE game_season SET current_season = ?1", [year])?;
    tx.execute("UPDATE game_season SET scheduled_season = ?1", [year - 1])?;
    tx.execute("UPDATE game_season SET current_round_seq = 1", [])?;
    tx.commit()?;
    Ok(())
}

fn process_games(num_of_games: i8) -> Result<()> {
    let mut game_service = GameService {
        repo: app_context().game_repository.clone(),
    };

    for _ in 0..num_of_games {
        if let Err(e) = game_service.process_game_round() {
            let error_msg = t!("error", "function" => "process_game");
            bail!("{}, {}", error_msg, e);
        }
        info!("1 game processed.");
        println!("1 game processed.");
    }
    println!(
        "{}",
        t!("game_rounds_processed", "num_of_rounds" => num_of_games.to_string())
    );
    Ok(())
}

fn schedule_games(num_of_schedules: u8) -> Result<()> {
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
    Ok(())
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Clear game data, reset to the current year, schedule a season, and process 40 rounds
    #[arg(short, long)]
    newyear: bool,

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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    const GAME_TABLES: [&str; 8] = [
        "count",
        "inning",
        "game",
        "player_game_batting",
        "player_game_entry",
        "player_game_fielding",
        "player_game_running",
        "player_game_pitching",
    ];

    fn populated_database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        for table in GAME_TABLES {
            conn.execute_batch(&format!(
                "CREATE TABLE {table} (id INTEGER); INSERT INTO {table} VALUES (1);"
            ))
            .unwrap();
        }
        conn.execute_batch(
            "CREATE TABLE game_season (
                current_season INTEGER, scheduled_season INTEGER, current_round_seq INTEGER
             );
             INSERT INTO game_season VALUES (2020, 2021, 42);
             CREATE TABLE player_info (id INTEGER);
             INSERT INTO player_info VALUES (1);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn reset_clears_game_data_and_sets_requested_year() {
        let mut conn = populated_database();
        reset_season(&mut conn, 2031).unwrap();

        for table in GAME_TABLES {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table} was not cleared");
        }
        let season: (i32, i32, i32) = conn
            .query_row("SELECT * FROM game_season", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(season, (2031, 2030, 1));
        let players: i64 = conn
            .query_row("SELECT COUNT(*) FROM player_info", [], |row| row.get(0))
            .unwrap();
        assert_eq!(players, 1);
    }

    #[test]
    fn reset_rolls_back_on_failure() {
        let mut conn = populated_database();
        conn.execute_batch(
            "CREATE TRIGGER reject_season_update BEFORE UPDATE OF scheduled_season
             ON game_season BEGIN SELECT RAISE(ABORT, 'test failure'); END;",
        )
        .unwrap();

        assert!(reset_season(&mut conn, 2031).is_err());
        for table in GAME_TABLES {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 1, "{table} deletion was not rolled back");
        }
        let year: i32 = conn
            .query_row("SELECT current_season FROM game_season", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(year, 2020);
    }

    #[test]
    fn newyear_flag_is_optional_and_accepts_long_and_short_forms() {
        assert!(!Args::try_parse_from(["saberbb"]).unwrap().newyear);
        for flag in ["--newyear", "-n"] {
            assert!(Args::try_parse_from(["saberbb", flag]).unwrap().newyear);
        }
    }
}
