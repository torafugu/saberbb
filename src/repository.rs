use super::shared::game::Game;
use super::shared::game::GameManager;
use anyhow::Result;
use directories::ProjectDirs;
// use rusqlite::Connection;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_rusqlite::{from_row, to_params_named};
use std::fs;

pub const ERROR_LOAD_GAME_MANAGER: &str = "An error occurred in load_game_manager()";
pub const ERROR_SAVE_GAME_MANAGER: &str = "An error occurred in save_game_manager()";
pub const ERROR_SAVE_GAME: &str = "An error occurred in save_game()";
const ERROR_NO_RECORD: &str = "No record found";
const ERROR_NO_DATA_DIR: &str = "The data directory is not found.";

fn get_db_path() -> Result<std::path::PathBuf> {
    let proj_dirs = ProjectDirs::from("jp", "cosmi", "statbb").expect(ERROR_NO_DATA_DIR);

    let data_dir = proj_dirs.data_dir();
    fs::create_dir_all(data_dir)?;

    Ok(data_dir.join("statbb.db"))
}

pub fn load_game_manager() -> Result<GameManager> {
    let db_path = get_db_path()?;
    let conn = Connection::open(&db_path)?;

    let mut stmt = conn.prepare("SELECT season, phase FROM game_manager LIMIT 1")?;
    let mut rows = serde_rusqlite::from_rows::<GameManager>(stmt.query([])?);

    let game_manager = rows
        .next()
        .ok_or_else(|| anyhow::anyhow!(ERROR_NO_RECORD))??;

    Ok(game_manager)
}

pub fn save_game_manager(game_manager: &GameManager) -> Result<()> {
    let db_path = get_db_path()?;
    let conn = Connection::open(&db_path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS game_manager (
            season INTEGER PRIMARY KEY,
            phase INTEGER NOT NULL
        )",
        [],
    )?;

    let params = to_params_named(&game_manager)?;
    conn.execute(
        "INSERT OR REPLACE INTO game_manager (season, phase) VALUES (:season, :phase)",
        params.to_slice().as_slice(),
    )?;
    Ok(())
}

pub fn save_game(game: &Game) -> Result<()> {
    let db_path = get_db_path()?;
    let conn = Connection::open(&db_path)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS game (
            seq INTEGER PRIMARY KEY,
            top_team_id INTEGER NOT NULL,
            bottom_team_id INTEGER NOT NULL
        )",
        [],
    )?;

    //let params = to_params_named(&game_manager)?;
    conn.execute(
        "INSERT OR REPLACE INTO game (seq, top_team_id, bottom_team_id) VALUES (?1, ?2, ?3)",
        params![game.seq, game.top_team.id, game.bottom_team.id],
    )?;

    Ok(())
}

pub fn save_game_game_schedule(game_manager: GameManager) -> Result<()> {
    let db_path = get_db_path()?;
    let conn = Connection::open(&db_path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS game_manager (
            season INTEGER PRIMARY KEY,
            phase INTEGER NOT NULL
        )",
        [],
    )?;

    let params = to_params_named(&game_manager)?;
    conn.execute(
        "INSERT OR REPLACE INTO game_manager (season, phase) VALUES (:season, :phase)",
        params.to_slice().as_slice(),
    )?;
    Ok(())
}
