use super::persistence_config::get_db_conn;
use crate::domain::game_service::GameRepository;
use crate::domain::shared::game::{Bases, Count, Game, GameRound, GameType, Inning};
use crate::domain::shared::player::Batter;
use crate::domain::shared::team::Team;
use crate::domain::shared::types::{BattingResult, InningType};
use crate::t;
use anyhow::{Context, Result};
use chrono::NaiveDate;
use rusqlite::types::{FromSql, FromSqlResult, ValueRef};
use rusqlite::{Connection, params};
use std::sync::Arc;

const SELECT_BATTER_SQL: &str = "SELECT id, name, mod_ba, mod_slg FROM batter WHERE team_id = ?1";

struct SqlGameType(GameType);
impl FromSql for SqlGameType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<GameType>().map(SqlGameType).map_err(|e| {
            eprintln!("{} {}: {:?}", t!("error_parse"), gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

struct SqlInningType(InningType);
impl FromSql for SqlInningType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let tb = value.as_str()?;

        tb.parse::<InningType>().map(SqlInningType).map_err(|e| {
            eprintln!("{} {}: {:?}", t!("error_parse"), tb, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

struct SqlBattingResult(BattingResult);
impl FromSql for SqlBattingResult {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let br = value.as_str()?;

        br.parse::<BattingResult>()
            .map(SqlBattingResult)
            .map_err(|e| {
                eprintln!("{} {}: {:?}", t!("error_parse"), br, e);
                rusqlite::types::FromSqlError::InvalidType
            })
    }
}

pub struct SqlGameRepository {
    pub pool: Connection,
}

impl GameRepository for SqlGameRepository {
    fn load_game_round_to_process(&self) -> Result<GameRound> {
        let mut game_round: GameRound = self.pool.query_row(
            "SELECT id, season, seq, date FROM game_round, game_season 
                WHERE current_season = season AND current_round_seq + 1 = seq",
            params![],
            |row| {
                Ok(GameRound {
                    id: row.get("id")?,
                    season: row.get("season")?,
                    seq: row.get("seq")?,
                    date: row.get("date")?,
                    games: Vec::new(),
                })
            },
        )?;

        game_round.games = load_games(&game_round)
            .context(t!("error", "function" => "load_game_round_to_process"))?;
        Ok(game_round)
    }

    fn save_game_round(&mut self, game_round: &GameRound) -> Result<()> {
        let tx = self.pool.transaction()?;

        for game in &game_round.games {
            let _ = tx.execute(
            "INSERT OR REPLACE INTO game (
                    game_round_id, id, planned_date, actual_date, away_team_id, home_team_id, game_type, away_point, home_point
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                game_round.id,
                game.id,
                game.planned_date,
                game.actual_date,
                game.away_team.id,
                game.home_team.id,
                game.game_type.to_string(),
                game.away_point,
                game.home_point
            ],
        )?;

            for inning in game.innings.iter() {
                let _ = tx.execute(
                    "INSERT OR REPLACE INTO inning (game_id, seq, tb, point
                ) VALUES (
                 ?1, ?2, ?3, ?4)",
                    params![game.id, inning.seq, inning.tb.to_string(), inning.point],
                )?;

                for count in inning.counts.iter() {
                    let _ = tx.execute(
                    "INSERT OR REPLACE INTO count (
                        game_id, inning_seq, inning_tb, seq,
                        is_first_runner, is_second_runner, is_third_runner, batter_id, result, point, out
                        ) VALUES (
                         ?1, ?2, ?3, ?4,
                         ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        game.id,
                        inning.seq,
                        inning.tb.to_string(),
                        count.seq,
                        count.bases.first,
                        count.bases.second,
                        count.bases.third,
                        count.batter.id,
                        count.result.to_string(),
                        count.point,
                        count.out
                    ],
                )?;
                }
            }
        }

        let _ = tx.execute(
            "UPDATE game_season SET current_round_seq = current_round_seq + 1",
            params![],
        )?;

        tx.commit()?;

        Ok(())
    }
}

pub fn load_processed_games(season: i16) -> Result<Vec<Game>> {
    let conn = get_db_conn()?;

    let mut stmt = conn.prepare(
        "SELECT id, season, seq, date 
            FROM game_round, game_season 
            WHERE season = ?1 AND current_round_seq >= seq ORDER BY seq DESC",
    )?;
    let game_rounds_iter = stmt.query_map([season], |row| {
        Ok(GameRound {
            id: row.get("id")?,
            season: row.get("season")?,
            seq: row.get("seq")?,
            date: row.get("date")?,
            games: Vec::new(),
        })
    })?;

    let processed_game_rounds: Vec<GameRound> = game_rounds_iter.collect::<Result<Vec<_>, _>>()?;
    let mut processed_games = Vec::new();
    for game_round in processed_game_rounds {
        let mut _loaded_games =
            load_games(&game_round).context(t!("error", "function" => "load_games"))?;
        for loaded_game in _loaded_games {
            processed_games.push(loaded_game);
        }
    }
    Ok(processed_games)
}

pub fn load_processed_seasons() -> Result<Vec<i16>> {
    let conn = get_db_conn()?;

    let mut stmt = conn.prepare(
        "select season from game_round, game_season WHERE current_season >= season group by season",
    )?;
    let season_iter = stmt.query_map([], |row| {
        let s: i16 = row.get("season")?;
        Ok(s)
    })?;

    let processed_seasons: Vec<i16> = season_iter.collect::<Result<Vec<_>, _>>()?;
    Ok(processed_seasons)
}

fn load_games(game_round: &GameRound) -> Result<Vec<Game>> {
    let conn = get_db_conn()?;
    let mut games: Vec<Game> = Vec::new();

    let mut stmt_game = conn.prepare(
        "SELECT 
                g.id,
                g.planned_date,
                g.actual_date,
                g.away_team_id, 
                t_away.name AS away_team_name,
                g.home_team_id, 
                t_home.name AS home_team_name,
                g.game_type,
                g.away_point,
                g.home_point
            FROM game g
            LEFT JOIN 
                Team t_away ON g.away_team_id = t_away.id
            LEFT JOIN 
                Team t_home ON g.home_team_id = t_home.id
            WHERE g.game_round_id = ?1
            ORDER BY g.id",
    )?;

    let games_iter = stmt_game.query_map([game_round.id], |row| {
        Ok(Game {
            id: row.get("id")?,
            planned_date: row.get("planned_date")?,
            actual_date: row.get("actual_date")?,
            away_team: Team {
                id: row.get("away_team_id")?,
                name: row.get("away_team_name")?,
            },
            home_team: Team {
                id: row.get("home_team_id")?,
                name: row.get("home_team_name")?,
            },
            game_type: row.get::<_, SqlGameType>("game_type")?.0,
            innings: Vec::new(),
            away_point: row.get("away_point")?,
            home_point: row.get("home_point")?,
            away_batters: Vec::new(),
            home_batters: Vec::new(),
        })
    })?;

    for game in games_iter {
        let mut game = game?;
        let mut stmt_away_batter = conn.prepare(SELECT_BATTER_SQL)?;
        let away_batter_iter = stmt_away_batter.query_map([game.away_team.id], |row| {
            Ok(Batter {
                id: row.get("id")?,
                name: row.get("name")?,
                mod_ba: row.get("mod_ba")?,
                mod_slg: row.get("mod_slg")?,
            })
        })?;

        let mut stmt_home_batter = conn.prepare(SELECT_BATTER_SQL)?;
        let home_batter_iter = stmt_home_batter.query_map([game.home_team.id], |row| {
            Ok(Batter {
                id: row.get("id")?,
                name: row.get("name")?,
                mod_ba: row.get("mod_ba")?,
                mod_slg: row.get("mod_slg")?,
            })
        })?;

        for away_batter in away_batter_iter {
            game.away_batters.push(away_batter?);
        }

        for home_batter in home_batter_iter {
            game.home_batters.push(home_batter?);
        }

        let mut stmt_inning = conn.prepare(
            "SELECT seq, tb, point FROM inning WHERE game_id = ?1 ORDER BY game_id ASC, seq ASC, tb DESC",
        )?;
        let inning_iter = stmt_inning.query_map([game.id], |row| {
            Ok(Inning {
                seq: row.get("seq")?,
                tb: row.get::<_, SqlInningType>("tb")?.0,
                counts: Vec::new(),
                point: row.get("point")?,
            })
        })?;

        let mut stmt_count = conn.prepare(
            "SELECT seq, is_first_runner, is_second_runner, is_third_runner, result, point, out, 
                id as batter_id, name as batter_name, mod_ba as batter_ba, mod_slg as batter_slg
                FROM count, batter 
                WHERE count.batter_id = batter.id AND 
                game_id = ?1 AND inning_seq = ?2 AND inning_tb = ?3",
        )?;
        for inning in inning_iter {
            let mut _inning = inning?;
            let count_iter = stmt_count.query_map(
                params![game.id, _inning.seq, _inning.tb.to_string()],
                |row| {
                    Ok(Count {
                        seq: row.get("seq")?,
                        bases: Bases {
                            first: row.get("is_first_runner")?,
                            second: row.get("is_second_runner")?,
                            third: row.get("is_third_runner")?,
                        },
                        result: row.get::<_, SqlBattingResult>("result")?.0,
                        batter: Arc::from(Batter::new(
                            row.get("batter_id")?,
                            &row.get::<_, String>("batter_name")?,
                            row.get("batter_ba")?,
                            row.get("batter_slg")?,
                        )),
                        point: row.get("point")?,
                        out: row.get("out")?,
                    })
                },
            )?;

            for count in count_iter {
                _inning.counts.push(count?);
            }

            game.innings.push(_inning);
        }
        games.push(game.clone());
    }

    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockRepo;
    impl GameRepository for MockRepo {
        fn save_game_round(&mut self, _round: &GameRound) -> Result<()> {
            Ok(())
        }
        fn load_game_round_to_process(&self) -> Result<GameRound> {
            let game_round = GameRound {
                id: 1,
                season: 2026,
                seq: 1,
                date: NaiveDate::parse_from_str("20260101", "%Y%m%d")?,
                games: Vec::new(),
            };
            Ok(game_round)
        }
    }

    #[test]
    fn test_load_processed_games_success() {
        let season = 2026;
        let result = load_processed_games(season);

        // 1. Assert the result is OK
        assert!(result.is_ok(), "Should return Ok for valid season");

        let games = result.unwrap();

        // 2. Assert number of the proceessed games and the season value in the game
        assert!(!games.is_empty(), "Games list should not be empty");
        for game in &games {
            assert!(game.id > 0);
        }
    }

    #[test]
    fn test_load_processed_games_empty_season() {
        let result = load_processed_games(9999); // Not exiting season 
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_load_processed_seasons_success() {
        let result = load_processed_seasons();

        // 1. Assert the result is OK
        assert!(result.is_ok(), "Should return Ok");

        let seasons = result.unwrap();

        // 2. Assert number of the proceessed games and the season value in the game
        assert!(!seasons.is_empty(), "Seasons list should not be empty");
        for season in seasons {
            assert!(season > 1900);
        }
    }
}
