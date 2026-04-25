use crate::domains::team::League;
use crate::domains::team::Team;

use super::persistence_config::get_db_conn;
use anyhow::Result;
use rusqlite::params;

pub fn load_all_leagues() -> Result<Vec<League>> {
    let conn = get_db_conn()?;

    let mut stmt_league = conn.prepare("SELECT id, name FROM league ORDER BY id")?;
    let league_iter = stmt_league.query_map([], |row| {
        Ok(League {
            id: row.get("id")?,
            name: row.get("name")?,
            teams: Vec::new(),
        })
    })?;

    let mut leagues: Vec<League> = Vec::new();

    for league in league_iter {
        let mut _league = league?;
        let mut stmt_team = conn.prepare("SELECT id, name FROM team WHERE league_id = ?1")?;
        let team_iter = stmt_team.query_map(params![_league.id], |row| {
            Ok(Team {
                id: row.get("id")?,
                name: row.get("name")?,
            })
        })?;

        for team in team_iter {
            _league.teams.push(team?);
        }

        leagues.push(_league);
    }

    Ok(leagues)
}
