use super::sql_types::{SqlBattingResult, SqlInningType};
use crate::domain::shared::game::{Count, Game, GameRound, Inning};
use crate::domain::shared::player::Player;
use crate::domain::shared::team::Team;
use crate::repositories::persistence_config::SqliteManager;
use crate::repositories::sql_types::SqlGameType;
use crate::t;
use anyhow::Result;
use deadpool::managed::Pool;
use rusqlite::params;
use std::sync::Arc;

pub trait GameRepository {
    fn save_game_round(&mut self, round: &GameRound) -> Result<()>;
    fn load_game_round_to_process(&self) -> Result<GameRound>;
    fn load_processed_games(&self, season: u16) -> Result<Vec<Game>>;
    fn load_processed_seasons(&self) -> Result<Vec<u16>>;
    fn load_games(&self, game_round: &GameRound) -> Result<Vec<Game>>;
    fn load_team_players(&self, team_id: u16) -> Result<Vec<Player>>;
    fn load_innings(&self, game_id: u32) -> Result<Vec<Inning>>;
    fn load_counts(&self, game_id: u32, inning: &Inning) -> Result<Vec<Count>>;
}

type DbPool = Pool<SqliteManager>;

#[derive(Clone)]
pub struct SqlGameRepository {
    pub pool: DbPool,
}
impl GameRepository for SqlGameRepository {
    fn load_game_round_to_process(&self) -> Result<GameRound> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let mut game_round: GameRound = conn.query_row(
            "SELECT id, season, seq, date
                    FROM game_season 
                    INNER JOIN game_round
                    ON current_season = season AND current_round_seq + 1 = seq",
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
            Ok(loaded_games) => {
                game_round.games = loaded_games;
            }
            Err(e) => {
                eprintln!("{}:{}", t!("error", "function" => "load_games"), e);
                return Err(e.into());
            }
        }
        Ok(game_round)
    }

    fn save_game_round(&mut self, game_round: &GameRound) -> Result<()> {
        let mut conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));
        let tx = conn.transaction()?;

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
           return Err(e.into());
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
                        game_id, inning_seq, inning_tb, seq, bases_occupied, 
                        pitcher_id, catcher_id, 
                        first_baseman_id, second_baseman_id, third_baseman_id, shortstop_id, 
                        left_fielder_id, center_fielder_id, right_fielder_id, 
                        batter_id, 
                        result, point, out
                        ) VALUES (
                         ?1, ?2, ?3, ?4, ?5, 
                         ?6, ?7, 
                         ?8, ?9, ?10, ?11, 
                         ?12, ?13, ?14, 
                         ?15,
                         ?16, ?17, ?18)",
                        params![
                            game.id,
                            inning.seq,
                            inning.tb,
                            count.seq,
                            count.bases_occupied,
                            count.pitcher.id,
                            count.catcher.id,
                            count.first_baseman.id,
                            count.second_baseman.id,
                            count.third_baseman.id,
                            count.shortstop.id,
                            count.left_fielder.id,
                            count.center_fielder.id,
                            count.right_fielder.id,
                            count.batter.id,
                            count.result,
                            count.point,
                            count.out
                        ],
                    ) {
                        eprintln!("{}:{}", t!("error", "SQL" => "INSERT INTO count"), e);
                        return Err(e.into());
                    };
                }
            }
        }

        if let Err(e) = tx.execute(
            "UPDATE game_season SET current_round_seq = current_round_seq + 1",
            params![],
        ) {
            eprintln!("{}:{}", t!("error", "SQL" => "UPDATE game_season"), e);
            return Err(e.into());
        };

        if let Err(e) = tx.commit() {
            eprintln!(
                "{}:{}",
                t!("error", "Function" => "commit of save_game_round"),
                e
            );
            return Err(e.into());
        };

        Ok(())
    }

    fn load_processed_games(&self, season: u16) -> Result<Vec<Game>> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));
        let mut stmt = conn.prepare(
            "SELECT id, season, seq, date 
                    FROM game_season 
                    INNER JOIN game_round
                    ON current_round_seq >= seq
                    WHERE season = ?1
                    ORDER BY seq DESC",
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

    fn load_processed_seasons(&self) -> Result<Vec<u16>> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));
        let mut stmt = conn.prepare("select season from game_round, game_season WHERE current_season >= season group by season")?;

        let seasons = stmt.query_map([], |row| {
            let s: u16 = row.get("season")?;
            Ok(s)
        });
        if let Err(e) = &seasons {
            eprintln!(
                "{}:{}",
                t!("error", "SQL" => "select season from game_round, game_season"),
                e
            );
        };

        let processed_seasons: Vec<u16> = seasons?.collect::<Result<Vec<_>, _>>()?;
        Ok(processed_seasons)
    }

    fn load_games(&self, game_round: &GameRound) -> Result<Vec<Game>> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));
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

            match self.load_team_players(game.away_team.id) {
                Ok(loaded_players) => {
                    game.away_team.players = loaded_players;
                }
                Err(e) => {
                    eprintln!(
                        "{}:{}",
                        t!("error", "function" => "load_team_players for away"),
                        e
                    );
                }
            }

            match self.load_team_players(game.home_team.id) {
                Ok(loaded_players) => {
                    game.home_team.players = loaded_players;
                }
                Err(e) => {
                    eprintln!(
                        "{}:{}",
                        t!("error", "function" => "load_team_players for home"),
                        e
                    );
                }
            }

            let mut innings = Vec::new();

            match self.load_innings(game.id) {
                Ok(loaded_innings) => {
                    innings = loaded_innings;
                }
                Err(e) => {
                    eprintln!("{}:{}", t!("error", "function" => "load_innings"), e);
                }
            }

            for inning in innings {
                let mut inning = inning;
                match self.load_counts(game.id, &inning) {
                    Ok(loaded_counts) => {
                        inning.counts = loaded_counts;
                    }
                    Err(e) => {
                        eprintln!("{}:{}", t!("error", "function" => "load_counts"), e);
                    }
                }
                game.innings.push(inning);
            }
            games.push(game.clone());
        }

        Ok(games)
    }

    fn load_team_players(&self, team_id: u16) -> Result<Vec<Player>> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let mut players: Vec<Player> = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id, first_name, last_name, mod_ba, mod_slg FROM player WHERE team_id = ?1",
        )?;
        let player_iter = stmt.query_map([team_id], |row| {
            Ok(Player::batter(
                row.get("id")?,
                &row.get::<_, String>("first_name")?,
                &row.get::<_, String>("last_name")?,
                row.get("mod_ba")?,
                row.get("mod_slg")?,
            ))
        })?;

        for player_result in player_iter {
            let player = player_result?;
            players.push(player);
        }
        Ok(players)
    }

    fn load_innings(&self, game_id: u32) -> Result<Vec<Inning>> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let mut innings = Vec::new();

        let mut stmt = conn.prepare("SELECT seq, tb, point FROM inning WHERE game_id = ?1 ORDER BY game_id ASC, seq ASC, tb DESC")?;
        let inning_iter = stmt.query_map([game_id], |row| {
            Ok(Inning {
                seq: row.get("seq")?,
                tb: row.get::<_, SqlInningType>("tb")?.0,
                counts: Vec::new(),
                point: row.get("point")?,
            })
        })?;

        for inning_result in inning_iter {
            let inning = inning_result?;
            innings.push(inning);
        }

        Ok(innings)
    }

    fn load_counts(&self, game_id: u32, inning: &Inning) -> Result<Vec<Count>> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let mut counts = Vec::new();

        let mut stmt = conn.prepare(
            "SELECT seq, bases_occupied, result, point, out, 
                b.id as b_id, b.first_name as b_first_name, b.last_name as b_last_name, b.mod_ba as ba, b.mod_slg as slg, 
                p.id as p_id, p.first_name as p_first_name, p.last_name as p_last_name,
                c.id as c_id, c.first_name as c_first_name, c.last_name as c_last_name,
                fb.id as fb_id, fb.first_name as fb_first_name, fb.last_name as fb_last_name,
                sb.id as sb_id, sb.first_name as sb_first_name, sb.last_name as sb_last_name,
                tb.id as tb_id, tb.first_name as tb_first_name, tb.last_name as tb_last_name,
                ss.id as ss_id, ss.first_name as ss_first_name, ss.last_name as ss_last_name,
                lf.id as lf_id, lf.first_name as lf_first_name, lf.last_name as lf_last_name,
                cf.id as cf_id, cf.first_name as cf_first_name, cf.last_name as cf_last_name,
                rf.id as rf_id, rf.first_name as rf_first_name, rf.last_name as rf_last_name
                FROM count
                INNER JOIN player AS b
                ON count.batter_id = b.id
                INNER JOIN player AS p
                ON count.pitcher_id = p.id
                INNER JOIN player AS c
                ON count.catcher_id = c.id
                INNER JOIN player AS fb
                ON count.first_baseman_id = fb.id
                INNER JOIN player AS sb
                ON count.second_baseman_id = sb.id
                INNER JOIN player AS tb
                ON count.third_baseman_id = tb.id
                INNER JOIN player AS ss
                ON count.shortstop_id = ss.id
                INNER JOIN player AS lf
                ON count.left_fielder_id = lf.id
                INNER JOIN player AS cf
                ON count.center_fielder_id = cf.id
                INNER JOIN player AS rf
                ON count.right_fielder_id = rf.id 
                WHERE game_id = ?1 AND inning_seq = ?2 AND inning_tb = ?3",
        )?;

        let count_iter = stmt.query_map(params![game_id, inning.seq, inning.tb], |row| {
            Ok(Count {
                seq: row.get("seq")?,
                bases_occupied: row.get("bases_occupied")?,
                result: row.get::<_, SqlBattingResult>("result")?.0,
                pitcher: Arc::from(Player::min(
                    row.get("p_id")?,
                    &row.get::<_, String>("p_first_name")?,
                    &row.get::<_, String>("p_last_name")?,
                )),
                catcher: Arc::from(Player::min(
                    row.get("c_id")?,
                    &row.get::<_, String>("c_first_name")?,
                    &row.get::<_, String>("c_last_name")?,
                )),
                first_baseman: Arc::from(Player::min(
                    row.get("fb_id")?,
                    &row.get::<_, String>("fb_first_name")?,
                    &row.get::<_, String>("fb_last_name")?,
                )),
                second_baseman: Arc::from(Player::min(
                    row.get("sb_id")?,
                    &row.get::<_, String>("sb_first_name")?,
                    &row.get::<_, String>("sb_last_name")?,
                )),
                third_baseman: Arc::from(Player::min(
                    row.get("tb_id")?,
                    &row.get::<_, String>("tb_first_name")?,
                    &row.get::<_, String>("tb_last_name")?,
                )),
                shortstop: Arc::from(Player::min(
                    row.get("ss_id")?,
                    &row.get::<_, String>("ss_first_name")?,
                    &row.get::<_, String>("ss_last_name")?,
                )),
                left_fielder: Arc::from(Player::min(
                    row.get("lf_id")?,
                    &row.get::<_, String>("lf_first_name")?,
                    &row.get::<_, String>("lf_last_name")?,
                )),
                center_fielder: Arc::from(Player::min(
                    row.get("cf_id")?,
                    &row.get::<_, String>("cf_first_name")?,
                    &row.get::<_, String>("cf_last_name")?,
                )),
                right_fielder: Arc::from(Player::min(
                    row.get("rf_id")?,
                    &row.get::<_, String>("rf_first_name")?,
                    &row.get::<_, String>("rf_last_name")?,
                )),
                batter: Arc::from(Player::batter(
                    row.get("b_id")?,
                    &row.get::<_, String>("b_first_name")?,
                    &row.get::<_, String>("b_last_name")?,
                    row.get("ba")?,
                    row.get("slg")?,
                )),
                point: row.get("point")?,
                out: row.get("out")?,
            })
        })?;

        for count_result in count_iter {
            let count = count_result?;
            counts.push(count);
        }

        Ok(counts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::persistence_config::get_sqlite_manager;

    #[test]
    fn test_load_processed_games_success() {
        let manager = get_sqlite_manager().expect(&t!("dbpool_failed"));
        let pool: DbPool = Pool::builder(manager).max_size(16).build().unwrap();
        let game_repository = SqlGameRepository { pool };

        let season = 2026;
        let result = game_repository.load_processed_games(season);

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
        let manager = get_sqlite_manager().expect(&t!("dbpool_failed"));
        let pool: DbPool = Pool::builder(manager).max_size(16).build().unwrap();
        let game_repository = SqlGameRepository { pool };

        let result = game_repository.load_processed_games(9999); // Not existing season

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_load_processed_seasons_success() {
        let manager = get_sqlite_manager().expect(&t!("dbpool_failed"));
        let pool: DbPool = Pool::builder(manager).max_size(16).build().unwrap();
        let game_repository = SqlGameRepository { pool };

        let result = game_repository.load_processed_seasons();

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
