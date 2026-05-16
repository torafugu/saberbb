use crate::domain::shared::player::Player;
use crate::domain::shared::team::Team;
use crate::repositories::persistence_config::SqliteManager;
use crate::t;
use anyhow::Result;
use deadpool::managed::Pool;
use rusqlite::{Error, params};

type DbPool = Pool<SqliteManager>;

pub trait PlayerRepository {
    fn save_player(&mut self, team: Team, player: Player) -> Result<()>;
    fn random_name(&self, language: String) -> Result<[String; 2]>;
    fn next_player_dist_team(&self) -> Result<Team>;
}

#[derive(Clone)]
pub struct SqlPlayerRepository {
    pub pool: DbPool,
}

impl PlayerRepository for SqlPlayerRepository {
    fn save_player(&mut self, team: Team, player: Player) -> Result<()> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        if let Err(e) = conn.execute(
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
        ) {
            eprintln!("{}:{}", t!("error", "SQL" => "INSERT INTO player"), e);
            return Err(e.into());
        };
        Ok(())
    }

    fn random_name(&self, language: String) -> Result<[String; 2]> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let mut names: [String; 2] = ["".to_string(), "".to_string()];

        // Retrieve First Name
        let res_count_first_names = conn.query_row(
            "SELECT COUNT(*) as count FROM first_names WHERE country = ?1 AND gender = 'M'",
            params![language],
            |row| Ok(row.get::<_, u32>("count")?),
        );

        let count_first_names = res_count_first_names?;
        if count_first_names == 0 {
            return Ok(names);
        }

        let mut random_offset = rand::random_range(0..count_first_names);
        let first_name = conn.query_row(
            "SELECT name FROM first_names WHERE country = ?1 AND gender = 'M' LIMIT 1 OFFSET ?2",
            params![language, random_offset],
            |row| Ok(row.get("name")?),
        );

        if first_name == Err(Error::QueryReturnedNoRows) {
            return Ok(names);
        } else if let Err(e) = first_name {
            eprintln!("{}:{}", t!("error", "SQL" => "SELECT FROM first_names"), e);
            return Err(e.into());
        }
        names[0] = first_name?;

        // Retrieve Last Name
        let res_count_last_names = conn.query_row(
            "SELECT COUNT(*) as count FROM last_names WHERE country = ?1",
            params![language],
            |row| Ok(row.get::<_, u32>("count")?),
        );

        let count_last_names = res_count_last_names?;
        if count_last_names == 0 {
            return Ok(names);
        }

        random_offset = rand::random_range(0..count_last_names);
        let last_name = conn.query_row(
            "SELECT name FROM last_names WHERE country = ?1 LIMIT 1 OFFSET ?2",
            params![language, random_offset],
            |row| Ok(row.get("name")?),
        );

        if last_name == Err(Error::QueryReturnedNoRows) {
            return Ok(names);
        } else if let Err(e) = last_name {
            eprintln!("{}:{}", t!("error", "SQL" => "SELECT FROM last_names"), e);
            return Err(e.into());
        }
        names[1] = last_name?;

        Ok(names)
    }

    fn next_player_dist_team(&self) -> Result<Team> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));
        let team = conn.query_row(
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
                    players: Vec::new(),
                })
            },
        );
        if let Err(e) = &team {
            eprintln!("{}:{}", t!("error", "SQL" => "SELECT FROM team"), e);
        }

        Ok(team?)
    }
}
