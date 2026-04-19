use super::super::shared::game::GameRound;
use super::persistence_config::get_db_conn;
use anyhow::Result;
use rusqlite::params;

pub const ERROR_SAVE_GAME_ROUNDS: &str = "An error occurred in save_game_rounds()";

pub fn save_game_rounds(game_rounds: Vec<GameRound>) -> Result<()> {
    let conn = get_db_conn()?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS game_round (
            season INTEGER,
            seq INTEGER, 
            date TEXT NOT NULL,
            PRIMARY KEY (season, seq)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS game_schedule (
            game_round_seq INTEGER, seq INTEGER, 
            away_team_id INTEGER, home_team_id INTEGER, game_type TEXT NOT NULL, 
            PRIMARY KEY (game_round_seq, seq, away_team_id, home_team_id)
        )",
        [],
    )?;

    for game_round in game_rounds {
        conn.execute(
            "INSERT OR REPLACE INTO game_round (season, seq, date) VALUES (?1, ?2, ?3)",
            params![game_round.season, game_round.seq, game_round.date],
        )?;

        for game_schedule in game_round.game_schedules {
            conn.execute(
                "INSERT OR REPLACE INTO game_schedule (game_round_seq, seq, away_team_id, home_team_id, game_type) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![game_round.seq, game_schedule.seq, game_schedule.away_team.id, game_schedule.home_team.id, game_schedule.game_type.to_string()],
            )?;
        }
    }

    Ok(())
}
