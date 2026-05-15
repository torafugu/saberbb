use crate::domain::shared::player::{BattingStats, Player};
use crate::domain::shared::team::{Standing, Team};
use crate::domain::statistics_service::StatRepository;
use anyhow::Result;
use rusqlite::Connection;

pub struct SqlStatRepository {
    pub pool: Connection,
}

impl StatRepository for SqlStatRepository {
    fn load_standings(&self) -> Result<Vec<Standing>> {
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
                    players: Vec::new(),
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

    fn load_batting_stats(&self) -> Result<Vec<BattingStats>> {
        let mut stmt = self.pool.prepare(
            "SELECT 
                        batter_id,
                        b.first_name AS batter_first_name,
                        b.last_name AS batter_last_name,
                        SUM(1) AS AB,
                        SUM(CASE WHEN result = 'Single' THEN 1 ELSE 0 END) AS single,
                        SUM(CASE WHEN result = 'Double' THEN 1 ELSE 0 END) AS double,
                        SUM(CASE WHEN result = 'Triple' THEN 1 ELSE 0 END) AS triple,
                        SUM(CASE WHEN result = 'HomeRun' THEN 1 ELSE 0 END) AS homeRun,
                        COALESCE(ROUND(CAST(SUM(CASE WHEN result IN ('Single', 'Double', 'Triple', 'Homerun') THEN 1 ELSE 0 END) AS REAL) / NULLIF(SUM(1), 0), 3), 0.0) AS BA,
                        SUM(point) AS rbi
                    FROM count
                    LEFT JOIN 
                        Player b ON count.batter_id = b.id
                    GROUP BY batter_id
                    ORDER BY batter_id",
        )?;

        let batting_stats_iter = stmt.query_map([], |row| {
            let first_name: String = row.get("batter_name")?;
            let last_name: String = row.get("batter_name")?;
            Ok(BattingStats {
                batter: Player::min(row.get("batter_id")?, &first_name, &last_name),
                ab: row.get("ab")?,
                single: row.get("single")?,
                double: row.get("double")?,
                triple: row.get("triple")?,
                homerun: row.get("homerun")?,
                ba: row.get("ba")?,
                rbi: row.get("rbi")?,
            })
        })?;

        let batting_stats: Vec<BattingStats> = batting_stats_iter.collect::<Result<Vec<_>, _>>()?;
        Ok(batting_stats)
    }
}
