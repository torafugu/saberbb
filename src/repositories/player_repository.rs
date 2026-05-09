use super::sql_types::SqlRL;
use crate::domain::player_service::PlayerRepository;
use crate::domain::shared::player::Player;
use crate::domain::shared::team::{League, Team};
use anyhow::Result;
use rusqlite::{Connection, params};

pub struct SqlPlayerRepository {
    pub pool: Connection,
}

impl PlayerRepository for SqlPlayerRepository {
    fn save_player(&mut self, team: Team, player: Player) -> Result<()> {
        self.pool.execute(
            "INSERT INTO player (
                        team_id, first_name, last_name,
                        age, throw, mod_speed, mod_control, bat, mod_ba, mod_slg
                        ) VALUES (
                         ?1, ?2, ?3,
                         ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                team.id,
                player.first_name,
                player.last_name,
                player.age,
                player.throw,
                player.mod_speed,
                player.mod_control,
                player.bat,
                player.mod_ba,
                player.mod_slg
            ],
        )?;
        Ok(())
    }

    fn random_name(&self, language: String) -> Result<[String; 2]> {
        let mut name: [String; 2] = ["".to_string(), "".to_string()];

        // Retrieve First Name
        let count_first_names: i64 = self.pool.query_row(
            "SELECT COUNT(*) FROM first_names WHERE country = ?1 AND gender = 'M'",
            params![language],
            |row| row.get(0),
        )?;

        if count_first_names == 0 {
            return Ok(name);
        }

        let mut random_offset = rand::random_range(0..count_first_names);
        name[0] = self.pool.query_row(
            "SELECT name FROM first_names WHERE country = ?1 AND gender = 'M' LIMIT 1 OFFSET ?2",
            params![language, random_offset],
            |row| Ok(row.get(0)?),
        )?;

        // Retrieve Last Name
        let count_last_names: i64 = self.pool.query_row(
            "SELECT COUNT(*) FROM last_names WHERE country = ?1",
            params![language],
            |row| row.get(0),
        )?;

        if count_last_names == 0 {
            return Ok(name);
        }

        random_offset = rand::random_range(0..count_last_names);
        name[1] = self.pool.query_row(
            "SELECT name FROM last_names WHERE country = ?1 LIMIT 1 OFFSET ?2",
            params![language, random_offset],
            |row| Ok(row.get(0)?),
        )?;

        Ok(name)
    }

    fn next_player_dist_team(&self) -> Result<Team> {
        let team = self.pool.query_row(
            "SELECT 
                        t.id AS team_id, 
                        t.name AS team_name,
                        COUNT(p.id) AS player_count
                    FROM team t
                    LEFT JOIN player p ON t.id = p.team_id
                    GROUP BY t.id
                    ORDER BY player_count, t.id
                    LIMIT 1;",
            params![],
            |row| {
                Ok(Team {
                    id: row.get("team_id")?,
                    name: row.get("team_name")?,
                })
            },
        )?;

        Ok(team)
    }
}
