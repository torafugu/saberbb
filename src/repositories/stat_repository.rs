use crate::domain::shared::team::{Standing, Team};
use crate::domain::stat_service::StatRepository;
use anyhow::Result;
use rusqlite::Connection;

pub struct SqlStatRepository {
    pub pool: Connection,
}

impl StatRepository for SqlStatRepository {
    fn load_stadings(&self) -> Result<Vec<Standing>> {
        let mut stmt = self.pool.prepare(
            "SELECT 
                    team_id,
                    team_name,
                    SUM(games) AS games,
                    SUM(CASE WHEN result = 'win' THEN 1 ELSE 0 END) AS wins,
                    SUM(CASE WHEN result = 'loss' THEN 1 ELSE 0 END) AS losses,
                    SUM(CASE WHEN result = 'draw' THEN 1 ELSE 0 END) AS draws,
                    COALESCE(ROUND(CAST(SUM(CASE WHEN result = 'win' THEN 1 ELSE 0 END) AS FLOAT) / NULLIF(SUM(games), 0), 3), 0.0) AS pct
                FROM (
                    SELECT 
                        home_team_id AS team_id,
                        t_home.name AS team_name,
                        CASE 
                            WHEN actual_date = '1900-01-01' THEN 0 ELSE 1
                        END AS games,
                        CASE 
                            WHEN home_point > away_point THEN 'win'
                            WHEN home_point < away_point THEN 'loss'
                            ELSE 'draw'
                        END AS result
                    FROM game
                    LEFT JOIN 
		                Team t_home ON game.home_team_id = t_home.id
    
                 UNION ALL
    
                    SELECT 
                        away_team_id AS team_id,
                        t_away.name AS team_name,
                        CASE 
                            WHEN actual_date = '1900-01-01' THEN 0 ELSE 1
                        END AS games,
                        CASE 
                            WHEN away_point > home_point THEN 'win'
                            WHEN away_point < home_point THEN 'loss'
                            ELSE 'draw'
                        END AS result
                    FROM game
                    LEFT JOIN 
		                Team t_away ON game.away_team_id = t_away.id
                ) AS combined_results
                GROUP BY team_id
                ORDER BY pct DESC, wins DESC;",
        )?;

        let standings_iter = stmt.query_map([], |row| {
            Ok(Standing {
                team: Team {
                    id: row.get("team_id")?,
                    name: row.get("team_name")?,
                },
                games: row.get("games")?,
                wins: row.get("wins")?,
                losses: row.get("losses")?,
                draws: row.get("draws")?,
                pct: row.get("pct")?,
                gb: 0.0,
                r: 0,
                ra: 0,
            })
        })?;
        let standings: Vec<Standing> = standings_iter.collect::<Result<Vec<_>, _>>()?;
        Ok(standings)
    }
}
