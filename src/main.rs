use anyhow::Result;
use chrono::{Datelike, Local};
use clap::Parser;
use saberbb::adapters::tui::top_tui::menu;
use saberbb::config::load_app_config;
use saberbb::i18n::I18nManager;
use saberbb::repositories::db::{SqlDb, maintenance, setup};
use saberbb::repositories::game_repository::SqlGameRepository;
use saberbb::repositories::player_repository::SqlPlayerRepository;
use saberbb::repositories::schedule_repository::SqlScheduleRepository;
use saberbb::repositories::statistics_repository::SqlStatRepository;
use saberbb::{AppContext, init_app_context, t};
use saberbb::{init_dirs, proj_dirs};
use std::backtrace::Backtrace;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    let args = Args::parse();
    if args.gui && !cfg!(feature = "desktop") {
        anyhow::bail!("GUI support is optional. Run: cargo run --features desktop -- --gui");
    }

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

    if args.setup {
        setup()?;
        println!("Database setup completed.");
    }

    let ctx = AppContext {
        game_repository: SqlGameRepository::new()?,
        player_repository: SqlPlayerRepository::new()?,
        schedule_repository: SqlScheduleRepository::new()?,
        statistics_repository: SqlStatRepository::new()?,
    };
    init_app_context(ctx);

    info!("game started.");

    if args.sampleyear {
        sampleyear()?;
    }

    // Game Process Mode
    if let Some(num_of_games) = args.process {
        run_games(num_of_games, false)?;
    }

    if let Some(num_of_games) = args.analysis {
        analysis_games(num_of_games)?;
    }

    // Player Generate Mode
    if let Some(num_of_players) = args.generate {
        let report = saberbb::application::management::generate_characters(num_of_players)?;
        println!("{} players generated; {} managers assigned.", report.players, report.managers);
    }

    // Game Schedule Generate Mode
    if let Some(num_of_schedules) = args.schedule {
        schedule_games(num_of_schedules)?;
    }

    // Execute VACUUM and PRAGMA optimize
    if args.maintenance {
        maintenance()?;
    }

    if args.gui {
        #[cfg(feature = "desktop")]
        saberbb::adapters::gui::launch();
    }

    // TUI mode
    if args.view {
        menu(cfg.clone()).map_err(|e| anyhow::anyhow!("{e:#}"))?;
    }

    Ok(())
}

fn sampleyear() -> Result<()> {
    let mut conn = SqlDb::new()?.get_conn()?;
    reset_season(&mut conn, Local::now().year())?;
    schedule_games(1)?;
    analysis_games(40)?;
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
         DELETE FROM player_game_fielding_candidate;
         DELETE FROM player_game_running;
         DELETE FROM player_game_pitching;
         DELETE FROM player_game_contact_state;
         DELETE FROM player_game_pitching_decision;",
    )?;
    tx.execute("UPDATE game_season SET current_season = ?1", [year])?;
    tx.execute("UPDATE game_season SET scheduled_season = ?1", [year - 1])?;
    tx.execute("UPDATE game_season SET current_round_seq = 1", [])?;
    tx.commit()?;
    Ok(())
}

fn analysis_games(num_of_games: i8) -> Result<()> {
    run_games(num_of_games, true)
}

fn run_games(num_of_games: i8, record_contact_states: bool) -> Result<()> {
    saberbb::application::management::process_rounds(num_of_games.max(0) as u16, record_contact_states)?;
    println!("{}", t!("game_rounds_processed", "num_of_rounds" => num_of_games.to_string()));
    Ok(())
}

fn schedule_games(num_of_schedules: u8) -> Result<()> {
    saberbb::application::management::schedule_seasons(num_of_schedules)?;
    println!("{}", t!("game_seasons_scheduled", "num_of_seasons" => num_of_schedules.to_string()));
    Ok(())
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Recreate database tables and load SQL seeds and name CSVs from migrations
    #[arg(long)]
    setup: bool,

    /// Clear game data, reset to the current year, schedule a season, and analyze 40 rounds
    #[arg(short = 'n', long)]
    sampleyear: bool,

    /// Process game rounds without detailed contact diagnostics (default: 1 round)
    #[arg(short, long, num_args = 0..=1, default_missing_value = "1", conflicts_with = "analysis")]
    process: Option<i8>,

    /// Process game rounds with detailed contact diagnostics (default: 1 round)
    #[arg(short, long, num_args = 0..=1, default_missing_value = "1")]
    analysis: Option<i8>,

    /// Generate players and fill all vacant team manager positions
    #[arg(short, long)]
    generate: Option<u16>,

    /// DB maintenance
    #[arg(short, long)]
    maintenance: bool,

    /// Schedule games
    #[arg(short, long)]
    schedule: Option<u8>,

    /// Open the management TUI (results and statistics are in --gui)
    #[arg(short, long, alias = "tui", conflicts_with = "gui")]
    view: bool,

    /// Open the Dioxus results/statistics GUI (requires --features desktop)
    #[arg(long)]
    gui: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    const GAME_TABLES: [&str; 11] = [
        "count",
        "inning",
        "game",
        "player_game_batting",
        "player_game_entry",
        "player_game_fielding",
        "player_game_fielding_candidate",
        "player_game_running",
        "player_game_pitching",
        "player_game_contact_state",
        "player_game_pitching_decision",
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
    fn setup_flag_can_be_combined_with_other_commands() {
        assert!(!Args::try_parse_from(["saberbb"]).unwrap().setup);
        let args =
            Args::try_parse_from(["saberbb", "--setup", "--generate", "100", "-s", "1"]).unwrap();
        assert!(args.setup);
        assert_eq!(args.generate, Some(100));
        assert_eq!(args.schedule, Some(1));
    }

    #[test]
    fn process_and_analysis_accept_optional_round_counts() {
        for flag in ["--process", "-p"] {
            let args = Args::try_parse_from(["saberbb", flag]).unwrap();
            assert_eq!(args.process, Some(1));
            assert_eq!(args.analysis, None);
            assert_eq!(Args::try_parse_from(["saberbb", flag, "5"]).unwrap().process, Some(5));
        }
        for flag in ["--analysis", "-a"] {
            let args = Args::try_parse_from(["saberbb", flag]).unwrap();
            assert_eq!(args.analysis, Some(1));
            assert_eq!(args.process, None);
            assert_eq!(Args::try_parse_from(["saberbb", flag, "5"]).unwrap().analysis, Some(5));
        }
        assert!(Args::try_parse_from(["saberbb", "--process", "--analysis"]).is_err());
    }

    #[test]
    fn gui_and_management_tui_are_separate_entrypoints() {
        assert!(Args::try_parse_from(["saberbb", "--tui"]).unwrap().view);
        assert!(Args::try_parse_from(["saberbb", "--gui"]).unwrap().gui);
        assert!(Args::try_parse_from(["saberbb", "--view", "--gui"]).is_err());
    }

    #[test]
    fn sampleyear_flag_is_optional_and_accepts_long_and_short_forms() {
        assert!(!Args::try_parse_from(["saberbb"]).unwrap().sampleyear);
        for flag in ["--sampleyear", "-n"] {
            assert!(Args::try_parse_from(["saberbb", flag]).unwrap().sampleyear);
        }
    }
}
