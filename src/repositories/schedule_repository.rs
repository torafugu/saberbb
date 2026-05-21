use crate::domain::shared::game::{GameScheduler, GameSeason};
use crate::domain::shared::team::{League, Team};
use crate::repositories::persistence_config::SqliteManager;
use crate::t;
use anyhow::Result;
use chrono::NaiveDate;
use deadpool::managed::Pool;
use rusqlite::{Error, params};

type DbPool = Pool<SqliteManager>;

pub trait ScheduleRepository {
    fn load_game_season(&self) -> Result<GameSeason>;
    fn load_all_leagues(&self) -> Result<Vec<League>>;
    fn save_game_schedules(&mut self, game_schedules: Vec<GameScheduler>) -> Result<()>;
    fn update_scheduled_season(&self) -> Result<()>;
}

#[derive(Clone)]
pub struct SqlScheduleRepository {
    pub pool: DbPool,
}

impl ScheduleRepository for SqlScheduleRepository {
    fn load_game_season(&self) -> Result<GameSeason> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let start_date = conn.query_row(
            "SELECT season_start_date, scheduled_season + 1 AS scheduled_season FROM game_season LIMIT 1",
            params![],
            |row| {
                Ok(GameSeason {
                    start_date: row.get("season_start_date")?,
                    season: row.get("scheduled_season")?,
                })
            },
        );
        if let Err(e) = &start_date {
            eprintln!("{}:{}", t!("error", "SQL" => "SELECT FROM game_season"), e);
        }
        Ok(start_date?)
    }

    fn load_all_leagues(&self) -> Result<Vec<League>> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let mut stmt_league = conn.prepare("SELECT id, name FROM league ORDER BY id")?;
        let league_iter = stmt_league.query_map([], |row| {
            Ok(League {
                id: row.get("id")?,
                name: row.get("name")?,
                teams: Vec::new(),
            })
        });
        if let Err(e) = &league_iter {
            eprintln!(
                "{}:{}",
                t!("error", "SQL" => "SELECT FROM game_round, game_season"),
                e
            );
        }

        let mut leagues: Vec<League> = Vec::new();

        for league in league_iter? {
            let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

            let mut league = league?;
            let mut stmt_team = conn.prepare("SELECT id, name FROM team WHERE league_id = ?1")?;
            let team_iter = stmt_team.query_map(params![league.id], |row| {
                Ok(Team {
                    id: row.get("id")?,
                    name: row.get("name")?,
                    players: Vec::new(),
                })
            });
            if let Err(e) = &team_iter {
                eprintln!("{}:{}", t!("error", "SQL" => "SELECT FROM team"), e);
            }

            for team in team_iter? {
                league.teams.push(team?);
            }

            leagues.push(league);
        }

        Ok(leagues)
    }

    fn save_game_schedules(&mut self, game_schedules: Vec<GameScheduler>) -> Result<()> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        for game_schedule in game_schedules {
            if let Err(e) = conn.execute(
                "INSERT INTO game (
                season, round_seq, seq, planned_date, away_team_id, home_team_id, game_type
                ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7
                 )",
                params![
                    game_schedule.season,
                    game_schedule.round_seq,
                    game_schedule.seq,
                    game_schedule.planned_date,
                    game_schedule.away_team.id,
                    game_schedule.home_team.id,
                    game_schedule.game_type,
                ],
            ) {
                eprintln!("{}:{}", t!("error", "SQL" => "INSERT INTO game"), e);
                return Err(e.into());
            };
        }

        Ok(())
    }

    fn update_scheduled_season(&self) -> Result<()> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        if let Err(e) = conn.execute(
            "Update game_season SET scheduled_season = scheduled_season + 1",
            params![],
        ) {
            eprintln!("{}:{}", t!("error", "SQL" => "Update game_season"), e);
            return Err(e.into());
        };
        Ok(())
    }
}

mod tests {
    use super::*;

    struct MockRepo;
    impl ScheduleRepository for MockRepo {
        fn load_game_season(&self) -> Result<GameSeason> {
            let game_season = GameSeason {
                start_date: NaiveDate::parse_from_str("20260101", "%Y%m%m")?,
                season: 2025,
            };
            Ok(game_season)
        }

        fn load_all_leagues(&self) -> Result<Vec<League>> {
            let leagues = Vec::new();
            Ok(leagues)
        }

        fn save_game_schedules(&mut self, _game_rounds: Vec<GameScheduler>) -> Result<()> {
            Ok(())
        }

        fn update_scheduled_season(&self) -> Result<()> {
            Ok(())
        }
    }
}
