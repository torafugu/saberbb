use super::persistence_config::get_db_conn;
use crate::shared::game::GameSeason;
use anyhow::Result;
use rusqlite::params;

pub const ERROR_LOAD_GAME_SEASON: &str = "An error occurred in get_game_season()";

pub fn get_game_season() -> Result<GameSeason> {
    let conn = get_db_conn()?;
    let game_season: GameSeason = conn.query_row(
        "SELECT season, start_date FROM system_constants LIMIT 1",
        params![],
        |row| {
            Ok(GameSeason {
                season: row.get("season")?,
                start_date: row.get("start_date")?,
            })
        },
    )?;
    Ok(game_season)
}
