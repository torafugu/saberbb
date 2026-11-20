use crate::domain::shared::game::{GameResult, GameSchedule};
use crate::domain::strategy::position_player_usage::PositionPlayerWorkload;
use chrono::NaiveDate;
use rusqlite::{Connection, Result, params};
use std::collections::BTreeMap;

pub(super) fn record_game(conn: &Connection, game: &GameResult) -> Result<()> {
    if game.position_player_workloads.is_empty() && !table_exists(conn)? {
        return Ok(());
    }
    conn.execute_batch(include_str!(
        "../../migrations/ddl/game_position_player_workload_table.sql"
    ))?;
    conn.execute(
        "DELETE FROM game_position_player_workload WHERE game_id=?1",
        [game.id],
    )?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO game_position_player_workload(game_id,player_id,fielding,running,batting)
         VALUES (?1,?2,?3,?4,?5)",
    )?;
    for (&id, work) in &game.position_player_workloads {
        insert.execute(params![
            game.id,
            id,
            work.fielding,
            work.running,
            work.batting
        ])?;
    }
    Ok(())
}

pub(super) fn load_before_game(
    conn: &Connection,
    game: &GameSchedule,
) -> Result<BTreeMap<i64, f64>> {
    let mut fatigue = BTreeMap::new();
    if !table_exists(conn)? {
        return Ok(fatigue);
    }
    let mut query = conn.prepare_cached(
        "SELECT w.player_id,g.actual_date,w.fielding,w.running,w.batting
         FROM player_info p JOIN game_position_player_workload w ON w.player_id=p.id
         JOIN game g ON g.id=w.game_id
         WHERE p.team_id IN (?1,?2) AND g.actual_date!='1900-01-01' AND g.id!=?5
           AND (g.actual_date,COALESCE(g.seq,0),g.id)<(?3,?4,?5)
         ORDER BY w.player_id,g.actual_date,COALESCE(g.seq,0),g.id",
    )?;
    let mut rows = query.query(params![
        game.away_team.id,
        game.home_team.id,
        game.planned_date,
        game.seq,
        game.id
    ])?;
    while let Some(row) = rows.next()? {
        let date: NaiveDate = row.get(1)?;
        let work = PositionPlayerWorkload {
            fielding: row.get(2)?,
            running: row.get(3)?,
            batting: row.get(4)?,
        };
        let remaining = work.remaining((game.planned_date - date).num_days());
        *fatigue.entry(row.get(0)?).or_default() += remaining;
    }
    Ok(fatigue)
}

pub fn clear(conn: &Connection) -> Result<()> {
    if table_exists(conn)? {
        conn.execute("DELETE FROM game_position_player_workload", [])?;
    }
    Ok(())
}

fn table_exists(conn: &Connection) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='game_position_player_workload')",
        [], |row| row.get(0),
    )
}
