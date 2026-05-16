use crate::domain::shared::game::{GameRound, GameSeason};
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
    fn save_scheduled_game_rounds(&mut self, game_rounds: Vec<GameRound>) -> Result<()>;
    fn load_last_game_round_id(&self) -> Result<u32>;
    fn load_last_game_id(&self) -> Result<u32>;
    fn update_scheduled_season(&self, scheduled_season: u16) -> Result<()>;
}

#[derive(Clone)]
pub struct SqlScheduleRepository {
    pub pool: DbPool,
}

impl ScheduleRepository for SqlScheduleRepository {
    fn load_game_season(&self) -> Result<GameSeason> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let game_season  = conn.query_row(
        "SELECT start_season, start_date, current_season, current_round_seq, scheduled_season FROM game_season LIMIT 1",
        params![],
        |row| {
            Ok(GameSeason {
                start_season: row.get("start_season")?,
                start_date: row.get("start_date")?,
                current_season: row.get("current_season")?,
                current_round_seq: row.get("current_round_seq")?,
                scheduled_season: row.get("scheduled_season")?,
            })
        },
    );
        if let Err(e) = &game_season {
            eprintln!("{}:{}", t!("error", "SQL" => "SELECT FROM game_season"), e);
        }
        Ok(game_season?)
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

    fn load_last_game_round_id(&self) -> Result<u32> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let res_round_id = conn.query_row(
            "SELECT id FROM game_round ORDER BY id DESC LIMIT 1",
            params![],
            |row| Ok(row.get("id")?),
        );

        // in case the number of rows = 0
        if res_round_id == Err(Error::QueryReturnedNoRows) {
            Ok(0)
        } else if let Err(e) = res_round_id {
            eprintln!("{}:{}", t!("error", "SQL" => "SELECT FROM game_round"), e);
            return Err(e.into());
        } else {
            Ok(res_round_id?)
        }
    }

    fn load_last_game_id(&self) -> Result<u32> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let res_game_id = conn.query_row(
            "SELECT id FROM game ORDER BY id DESC LIMIT 1",
            params![],
            |row| Ok(row.get("id")?),
        );

        // in case the number of rows = 0
        if res_game_id == Err(Error::QueryReturnedNoRows) {
            Ok(0)
        } else {
            Ok(res_game_id?)
        }
    }

    fn save_scheduled_game_rounds(&mut self, game_rounds: Vec<GameRound>) -> Result<()> {
        let mut conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let tx = conn.transaction()?;

        for game_round in game_rounds {
            if let Err(e) = tx.execute(
                "INSERT OR REPLACE INTO game_round (id, season, seq, date) VALUES (?1, ?2, ?3, ?4)",
                params![
                    game_round.id,
                    game_round.season,
                    game_round.seq,
                    game_round.date
                ],
            ) {
                eprintln!("{}:{}", t!("error", "SQL" => "INSERT INTO game_round"), e);
                return Err(e.into());
            }

            for game in game_round.games {
                if let Err(e) = tx.execute(
                    "INSERT OR REPLACE INTO game (
                game_round_id, id, planned_date, actual_date, away_team_id, home_team_id, game_type, away_point, home_point
                ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9
                 )",
                    params![
                        game_round.id,
                        game.id,
                        game.planned_date,
                        game.actual_date,
                        game.away_team.id,
                        game.home_team.id,
                        game.game_type,
                        0,
                        0
                    ],
                ) {
                    eprintln!("{}:{}", t!("error", "SQL" => "INSERT INTO game"), e);
                    return Err(e.into());
                };
            }
        }

        if let Err(e) = tx.commit() {
            eprintln!(
                "{}:{}",
                t!("error", "Function" => "commit of save_scheduled_game_rounds"),
                e
            );
            return Err(e.into());
        };

        Ok(())
    }

    fn update_scheduled_season(&self, scheduled_season: u16) -> Result<()> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        if let Err(e) = conn.execute(
            "Update game_season SET scheduled_season = ?1",
            params![scheduled_season],
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
                start_season: 2026,
                start_date: NaiveDate::parse_from_str("20260101", "%Y%m%m")?,
                current_season: 2026,
                current_round_seq: 1,
                scheduled_season: 2025,
            };
            Ok(game_season)
        }

        fn load_all_leagues(&self) -> Result<Vec<League>> {
            let leagues = Vec::new();
            Ok(leagues)
        }

        fn load_last_game_round_id(&self) -> Result<u32> {
            Ok(1)
        }

        fn load_last_game_id(&self) -> Result<u32> {
            Ok(1)
        }

        fn save_scheduled_game_rounds(&mut self, _game_rounds: Vec<GameRound>) -> Result<()> {
            Ok(())
        }

        fn update_scheduled_season(&self, _updated_sheduled_season: u16) -> Result<()> {
            Ok(())
        }
    }
}
