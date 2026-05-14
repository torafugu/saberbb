use super::persistence_config::get_db_conn;
use super::sql_types::{SqlBattingResult, SqlInningType};
use crate::domain::game_service::{GameRepository, GameService};
use crate::domain::shared::game::{Bases, Count, Game, GameRound, Inning};
use crate::domain::shared::player::Player;
use crate::domain::shared::team::Team;
use crate::i18n::I18nManager;
use crate::repositories::sql_types::SqlGameType;
use crate::t;
use anyhow::Result;
use chrono::NaiveDate;
use rusqlite::{Connection, params};
use std::sync::Arc;

const SELECT_PLAYER_SQL: &str =
    "SELECT id, first_name, last_name, mod_ba, mod_slg FROM player WHERE team_id = ?1";

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

        match self.load_games(&game_round) {
            Ok(load_games) => {
                game_round.games = load_games;
            }
            Err(e) => {
                eprintln!("{}:{}", t!("error", "function" => "load_games"), e);
            }
        }
        Ok(game_round)
    }

    fn save_game_round(&mut self, game_round: &GameRound) -> Result<()> {
        let tx = self.pool.transaction()?;

        for game in &game_round.games {
            if let Err(e) = tx.execute(
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
                game.game_type,
                game.away_point,
                game.home_point
            ],
        ) {
           eprintln!("{}:{}", t!("error", "SQL" => "INSERT INTO game"), e); 
        };

            for inning in game.innings.iter() {
                if let Err(e) = tx.execute(
                    "INSERT OR REPLACE INTO inning (game_id, seq, tb, point
                ) VALUES (
                 ?1, ?2, ?3, ?4)",
                    params![game.id, inning.seq, inning.tb, inning.point],
                ) {
                    eprintln!("{}:{}", t!("error", "SQL" => "INSERT INTO inning"), e);
                };

                for count in inning.counts.iter() {
                    if let Err(e) = tx.execute(
                    "INSERT OR REPLACE INTO count (
                        game_id, inning_seq, inning_tb, seq,
                        is_first_runner, is_second_runner, is_third_runner, batter_id, result, point, out
                        ) VALUES (
                         ?1, ?2, ?3, ?4,
                         ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        game.id,
                        inning.seq,
                        inning.tb,
                        count.seq,
                        count.bases.first,
                        count.bases.second,
                        count.bases.third,
                        count.batter.id,
                        count.result,
                        count.point,
                        count.out
                    ],
                ) {
                    eprintln!("{}:{}", t!("error", "SQL" => "INSERT INTO count"), e); 
                };
                }
            }
        }

        if let Err(e) = tx.execute(
            "UPDATE game_season SET current_round_seq = current_round_seq + 1",
            params![],
        ) {
            eprintln!("{}:{}", t!("error", "SQL" => "UPDATE game_season"), e);
        };

        tx.commit()?;

        Ok(())
    }

    fn load_processed_games(&self, season: i16) -> Result<Vec<Game>> {
        let mut stmt = self.pool.prepare(
            "SELECT id, season, seq, date 
            FROM game_round, game_season 
            WHERE season = ?1 AND current_round_seq >= seq ORDER BY seq DESC",
        )?;

        let game_rounds = stmt.query_map([season], |row| {
            Ok(GameRound {
                id: row.get("id")?,
                season: row.get("season")?,
                seq: row.get("seq")?,
                date: row.get("date")?,
                games: Vec::new(),
            })
        });
        if let Err(e) = &game_rounds {
            eprintln!(
                "{}:{}",
                t!("error", "SQL" => "SELECT FROM game_round, game_season"),
                e
            );
        }

        let processed_game_rounds: Vec<GameRound> = game_rounds?.collect::<Result<Vec<_>, _>>()?;
        let mut processed_games = Vec::new();
        for game_round in processed_game_rounds {
            match self.load_games(&game_round) {
                Ok(loaded_games) => {
                    for loaded_game in loaded_games {
                        processed_games.push(loaded_game);
                    }
                }
                Err(e) => {
                    eprintln!("{}:{}", t!("error", "function" => "load_games"), e);
                }
            }
        }
        Ok(processed_games)
    }

    fn load_processed_seasons(&self) -> Result<Vec<i16>> {
        let mut stmt = self.pool.prepare("select season from game_round, game_season WHERE current_season >= season group by season")?;

        let seasons = stmt.query_map([], |row| {
            let s: i16 = row.get("season")?;
            Ok(s)
        });
        if let Err(e) = &seasons {
            eprintln!(
                "{}:{}",
                t!("error", "SQL" => "select season from game_round, game_season"),
                e
            );
        };

        let processed_seasons: Vec<i16> = seasons?.collect::<Result<Vec<_>, _>>()?;
        Ok(processed_seasons)
    }

    fn load_games(&self, game_round: &GameRound) -> Result<Vec<Game>> {
        let mut games: Vec<Game> = Vec::new();

        let mut stmt_game = self.pool.prepare(
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
                    players: Vec::new(),
                },
                home_team: Team {
                    id: row.get("home_team_id")?,
                    name: row.get("home_team_name")?,
                    players: Vec::new(),
                },
                game_type: row.get::<_, SqlGameType>("game_type")?.0,
                innings: Vec::new(),
                away_point: row.get("away_point")?,
                home_point: row.get("home_point")?,
            })
        });
        if let Err(e) = &games_iter {
            eprintln!("{}:{}", t!("error", "SQL" => "games_iter"), e);
        };

        for game in games_iter? {
            let mut game = game?;
            let mut stmt_away_player = self.pool.prepare(SELECT_PLAYER_SQL)?;
            let away_player_iter = stmt_away_player.query_map([game.away_team.id], |row| {
                Ok(Player::batter(
                    row.get("id")?,
                    &I18nManager::global().full_name(
                        &row.get::<_, String>("first_name")?,
                        &row.get::<_, String>("last_name")?,
                    ),
                    row.get("mod_ba")?,
                    row.get("mod_slg")?,
                ))
            });
            if let Err(e) = &away_player_iter {
                eprintln!("{}:{}", t!("error", "SQL" => "away_player_iter"), e);
            };

            for away_player in away_player_iter? {
                game.away_team.players.push(away_player?);
            }

            let mut stmt_home_player = self.pool.prepare(SELECT_PLAYER_SQL)?;
            let home_player_iter = stmt_home_player.query_map([game.home_team.id], |row| {
                Ok(Player::batter(
                    row.get("id")?,
                    &I18nManager::global().full_name(
                        &row.get::<_, String>("first_name")?,
                        &row.get::<_, String>("last_name")?,
                    ),
                    row.get("mod_ba")?,
                    row.get("mod_slg")?,
                ))
            });
            if let Err(e) = &home_player_iter {
                eprintln!("{}:{}", t!("error", "SQL" => "home_player_iter"), e);
            };

            for home_player in home_player_iter? {
                game.home_team.players.push(home_player?);
            }

            let mut stmt_inning = self.pool.prepare("SELECT seq, tb, point FROM inning WHERE game_id = ?1 ORDER BY game_id ASC, seq ASC, tb DESC")?;
            let inning_iter = stmt_inning.query_map([game.id], |row| {
                Ok(Inning {
                    seq: row.get("seq")?,
                    tb: row.get::<_, SqlInningType>("tb")?.0,
                    counts: Vec::new(),
                    point: row.get("point")?,
                })
            });
            if let Err(e) = &inning_iter {
                eprintln!(
                    "{}:{}",
                    t!("error", "SQL" => "SELECT seq, tb, point FROM inning"),
                    e
                );
            };

            let mut stmt_count = self.pool.prepare(
            "SELECT seq, is_first_runner, is_second_runner, is_third_runner, result, point, out, 
                id as batter_id, first_name, last_name, mod_ba as batter_ba, mod_slg as batter_slg
                FROM count, player 
                WHERE count.batter_id = player.id AND 
                game_id = ?1 AND inning_seq = ?2 AND inning_tb = ?3",
        )?;
            for inning in inning_iter? {
                let mut _inning = inning?;
                let count_iter =
                    stmt_count.query_map(params![game.id, _inning.seq, _inning.tb], |row| {
                        Ok(Count {
                            seq: row.get("seq")?,
                            bases: Bases {
                                first: row.get("is_first_runner")?,
                                second: row.get("is_second_runner")?,
                                third: row.get("is_third_runner")?,
                            },
                            result: row.get::<_, SqlBattingResult>("result")?.0,
                            batter: Arc::from(Player::batter(
                                row.get("batter_id")?,
                                &I18nManager::global().full_name(
                                    &row.get::<_, String>("first_name")?,
                                    &row.get::<_, String>("last_name")?,
                                ),
                                row.get("batter_ba")?,
                                row.get("batter_slg")?,
                            )),
                            point: row.get("point")?,
                            out: row.get("out")?,
                        })
                    });

                if let Err(e) = &count_iter {
                    eprintln!("{}:{}", t!("error", "SQL" => "count_iter"), e);
                };

                for count in count_iter? {
                    _inning.counts.push(count?);
                }

                game.innings.push(_inning);
            }
            games.push(game.clone());
        }

        Ok(games)
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::shared::game::GameType;

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
        fn load_processed_games(&self, _season: i16) -> Result<Vec<Game>> {
            let mut games: Vec<Game> = Vec::new();
            let game = Game {
                id: 1,
                planned_date: NaiveDate::parse_from_str("20250101", "%Y%m%d")?,
                actual_date: NaiveDate::parse_from_str("20250101", "%Y%m%d")?,
                away_team: Team {
                    id: 1,
                    name: "AAA".into(),
                    players: Vec::new(),
                },
                home_team: Team {
                    id: 2,
                    name: "BBB".into(),
                    players: Vec::new(),
                },
                game_type: GameType::Regular,
                innings: Vec::new(),
                away_point: 2,
                home_point: 3,
            };
            games.push(game);
            Ok(games)
        }
        fn load_processed_seasons(&self) -> Result<Vec<i16>> {
            let mut seasons: Vec<i16> = Vec::new();
            seasons.push(2026);
            Ok(seasons)
        }
        fn load_games(&self, _game_round: &GameRound) -> Result<Vec<Game>> {
            let mut games: Vec<Game> = Vec::new();
            let game = Game {
                id: 1,
                planned_date: NaiveDate::parse_from_str("20250101", "%Y%m%d")?,
                actual_date: NaiveDate::parse_from_str("20250101", "%Y%m%d")?,
                away_team: Team {
                    id: 1,
                    name: "AAA".into(),
                    players: Vec::new(),
                },
                home_team: Team {
                    id: 2,
                    name: "BBB".into(),
                    players: Vec::new(),
                },
                game_type: GameType::Regular,
                innings: Vec::new(),
                away_point: 2,
                home_point: 3,
            };
            games.push(game);
            Ok(games)
        }
    }

    #[test]
    fn test_load_processed_games_success() {
        let db_repo = SqlGameRepository {
            pool: get_db_conn().unwrap(),
        };
        let season = 2026;
        let result = db_repo.load_processed_games(season);

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
        let db_repo = SqlGameRepository {
            pool: get_db_conn().unwrap(),
        };
        let result = db_repo.load_processed_games(9999); // Not exiting season 
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_load_processed_seasons_success() {
        let db_repo = SqlGameRepository {
            pool: get_db_conn().unwrap(),
        };
        let result = db_repo.load_processed_seasons();

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
