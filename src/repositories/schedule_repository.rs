use super::persistence_config::get_db_conn;
use crate::domain::schedule_service::ScheduleRepository;
use crate::domain::shared::game::{GameRound, GameSeason};
use crate::domain::shared::team::{League, Team};
use anyhow::Result;
use chrono::NaiveDate;
use rusqlite::{Connection, Error, params};

pub struct SqlScheduleRepository {
    pub pool: Connection,
}

impl ScheduleRepository for SqlScheduleRepository {
    fn load_game_season(&self) -> Result<GameSeason> {
        let game_season: GameSeason = self.pool.query_row(
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
    )?;
        Ok(game_season)
    }

    fn load_all_leagues(&self) -> Result<Vec<League>> {
        let mut stmt_league = self
            .pool
            .prepare("SELECT id, name FROM league ORDER BY id")?;
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
            let mut stmt_team = self
                .pool
                .prepare("SELECT id, name FROM team WHERE league_id = ?1")?;
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

    fn load_last_game_round_id(&self) -> Result<i32> {
        let res_round_id = self.pool.query_row(
            "SELECT id FROM game_round ORDER BY id DESC LIMIT 1",
            params![],
            |row| Ok(row.get("id")?),
        );

        // in case the number of rows = 0
        if res_round_id == Err(Error::QueryReturnedNoRows) {
            Ok(0)
        } else {
            Ok(res_round_id?)
        }
    }

    fn load_last_game_id(&self) -> Result<i32> {
        let res_game_id = self.pool.query_row(
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
        let tx = self.pool.transaction()?;

        for game_round in game_rounds {
            let _ = tx.execute(
                "INSERT OR REPLACE INTO game_round (id, season, seq, date) VALUES (?1, ?2, ?3, ?4)",
                params![
                    game_round.id,
                    game_round.season,
                    game_round.seq,
                    game_round.date
                ],
            )?;

            for game in game_round.games {
                let _ = tx.execute(
                    "INSERT OR REPLACE INTO game (
                game_round_id, id, date, away_team_id, home_team_id, game_type, away_point, home_point
                ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8
                 )",
                    params![
                        game_round.id,
                        game.id,
                        game.date,
                        game.away_team.id,
                        game.home_team.id,
                        game.game_type.to_string(),
                        0,
                        0
                    ],
                )?;
            }
        }

        tx.commit()?;

        Ok(())
    }

    fn update_scheduled_season(&self, updated_sheduled_season: i16) -> Result<()> {
        self.pool.execute(
            "Update game_season SET scheduled_season = ?1",
            params![updated_sheduled_season],
        )?;
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

        fn load_last_game_round_id(&self) -> Result<i32> {
            Ok(1)
        }

        fn load_last_game_id(&self) -> Result<i32> {
            Ok(1)
        }

        fn save_scheduled_game_rounds(&mut self, game_rounds: Vec<GameRound>) -> Result<()> {
            Ok(())
        }

        fn update_scheduled_season(&self, updated_sheduled_season: i16) -> Result<()> {
            Ok(())
        }
    }
}
