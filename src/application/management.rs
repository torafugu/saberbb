//! Management use cases shared by CLI, TUI, and GUI.
use anyhow::{Context, Result};
use crate::app_context;
use crate::domain::{game_service::GameService, manager_service::ManagerService,
    player_factory::PlayerFactory, player_service::PlayerService,
    random_provider::RealRng, schedule_service::ScheduleService};
use crate::repositories::manager_repository::SqlManagerRepository;
use crate::I18nManager;

#[derive(Clone, Debug)]
pub struct GenerationReport { pub players: u16, pub managers: usize }

pub fn generate_characters(players: u16) -> Result<GenerationReport> {
    generate_characters_with_progress(players, |_| {})
}

pub fn generate_characters_with_progress(players: u16, mut progress: impl FnMut(u16)) -> Result<GenerationReport> {
    if players > 0 {
        let service = PlayerService { repo: app_context().player_repository.clone() };
        let mut factory = PlayerFactory::new(service);
        factory.load_player_probs()?;
        for index in 0..players {
            factory.generate_and_save_player()
                .with_context(|| format!("player generation failed after {index} saved players"))?;
            progress(index + 1);
        }
    }
    let mut managers = ManagerService { repo: SqlManagerRepository::new()? };
    let managers = managers.generate_missing_managers(&I18nManager::global().lang_db(), &mut RealRng::new())?;
    Ok(GenerationReport { players, managers })
}

pub fn schedule_seasons(seasons: u8) -> Result<()> {
    schedule_seasons_with_progress(seasons, |_| {})
}

pub fn schedule_seasons_with_progress(seasons: u8, mut progress: impl FnMut(u16)) -> Result<()> {
    let mut service = ScheduleService { repo: app_context().schedule_repository.clone() };
    for index in 0..seasons {
        service.schedule_season().with_context(|| format!("scheduling failed after {index} saved seasons"))?;
        progress(u16::from(index) + 1);
    }
    Ok(())
}

pub fn process_rounds(rounds: u16, analysis: bool) -> Result<()> {
    process_rounds_with_progress(rounds, analysis, |_| {})
}

pub fn process_rounds_with_progress(rounds: u16, analysis: bool, mut progress: impl FnMut(u16)) -> Result<()> {
    let mut service = GameService { repo: app_context().game_repository.clone() };
    let db = crate::repositories::db::DbClient::new()?;
    for index in 0..rounds {
        let mut conn = db.get_conn()?;
        prepare_next_round(&mut conn)
            .with_context(|| format!("processing failed after {index} completed rounds"))?;
        drop(conn);
        let result = if analysis { service.analyze_game_round() } else { service.process_game_round() };
        result.with_context(|| format!("processing failed after {index} completed rounds"))?;
        progress(index + 1);
    }
    Ok(())
}

// Scheduling may add a future season while the active season is exhausted (or
// still the initial seed year). Select its first unfinished round rather than
// reporting success for an empty round and advancing the cursor without games.
fn prepare_next_round(conn: &mut rusqlite::Connection) -> Result<()> {
    let status = crate::domain::holdout_schedule::inspect(conn)?;
    anyhow::ensure!(status.past_pending_games == 0, "Unprocessed games remain before the active season; inspect the season cursor before continuing.");
    let next = status.pending_rounds.first().context("No scheduled games remain. Create a schedule first.")?;
    if (next.season, next.round_seq) != (status.current_season, status.current_round_seq) {
        crate::domain::holdout_schedule::align_cursor(conn, (status.current_season, status.current_round_seq), next)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn processing_requires_pending_games_and_aligns_future_seasons() {
        let mut conn=rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE game_season(current_season INTEGER,current_round_seq INTEGER,scheduled_season INTEGER); INSERT INTO game_season VALUES(2025,1,2026); CREATE TABLE game(id INTEGER, season INTEGER, round_seq INTEGER, actual_date TEXT);").unwrap();
        assert!(prepare_next_round(&mut conn).is_err());
        assert_eq!(conn.query_row("SELECT current_round_seq FROM game_season",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        conn.execute_batch("INSERT INTO game VALUES(1,2026,1,NULL);").unwrap();
        prepare_next_round(&mut conn).unwrap();
        assert_eq!(conn.query_row("SELECT current_season FROM game_season",[],|r|r.get::<_,i64>(0)).unwrap(),2026);
        conn.execute_batch("INSERT INTO game VALUES(2,2025,1,NULL);").unwrap();
        assert!(prepare_next_round(&mut conn).is_err());
    }
}
