use super::persistence_config::get_db_conn;
use crate::domain::shared::game::GameRound;
use anyhow::Result;
use rusqlite::params;

pub fn save_scheduled_game_rounds(game_rounds: Vec<GameRound>) -> Result<()> {
    let conn = get_db_conn()?;

    for game_round in game_rounds {
        conn.execute(
            "INSERT OR REPLACE INTO game_round (season, seq, date) VALUES (?1, ?2, ?3)",
            params![game_round.season, game_round.seq, game_round.date],
        )?;

        for game in game_round.games {
            conn.execute(
                "INSERT OR REPLACE INTO game (
                game_round_season, game_round_seq, seq, date, away_team_id, home_team_id, game_type
                ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7
                 )",
                params![
                    game_round.season,
                    game_round.seq,
                    game.seq,
                    game.date,
                    game.away_team.id,
                    game.home_team.id,
                    game.game_type.to_string()
                ],
            )?;
        }
    }
    Ok(())
}
