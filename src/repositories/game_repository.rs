use super::persistence_config::get_db_conn;
use crate::domains::game::{Count, Game, GameRound, GameSeason, GameType, Inning};
use crate::domains::player::Batter;
use crate::domains::team::Team;
use crate::domains::types::{BattingResult, InningType};
use crate::t;
use anyhow::{Context, Result};
use rusqlite::params;
use rusqlite::types::{FromSql, FromSqlResult, ValueRef};
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

pub fn load_processed_games(season: i16) -> Result<Vec<Game>> {
    let conn = get_db_conn()?;

    let mut stmt = conn.prepare(
        "SELECT season, seq, date 
            FROM game_round, game_season 
            WHERE season = ?1 AND current_round_seq >= seq ORDER BY seq DESC",
    )?;
    let game_rounds_iter = stmt.query_map([season], |row| {
        Ok(GameRound {
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

pub fn load_game_season() -> Result<GameSeason> {
    let game_season: GameSeason = get_db_conn()?.query_row(
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

pub fn update_scheduled_season(updated_sheduled_season: i16) -> Result<()> {
    get_db_conn()?.execute(
        "Update game_season SET scheduled_season = ?1",
        params![updated_sheduled_season],
    )?;
    Ok(())
}

pub fn load_game_round_to_process() -> Result<GameRound> {
    let conn = get_db_conn()?;
    let mut game_round: GameRound = conn.query_row(
        "SELECT season, seq, date FROM game_round, game_season 
                WHERE current_season = season AND current_round_seq = seq",
        params![],
        |row| {
            Ok(GameRound {
                season: row.get("season")?,
                seq: row.get("seq")?,
                date: row.get("date")?,
                games: Vec::new(),
            })
        },
    )?;

    game_round.games = load_games(&game_round).context(t!("error", "function" => "load_games"))?;
    Ok(game_round)
}

pub fn save_game_round(game_round: &GameRound) -> Result<()> {
    let conn = get_db_conn()?;

    for game in &game_round.games {
        conn.execute(
            "INSERT OR REPLACE INTO game (
                    game_round_season, game_round_seq, seq, date, away_team_id, home_team_id, game_type
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                game_round.season,
                game_round.seq,
                game.seq,
                game.date,
                game.away_team.id,
                game.home_team.id,
                game.game_type.to_string()
            ],
        )?;

        for inning in game.innings.iter() {
            conn.execute(
                "INSERT OR REPLACE INTO inning (
                game_round_season, game_round_seq, game_seq, seq, tb, point
                ) VALUES (
                 ?1, ?2, ?3, ?4,  ?5, ?6)",
                params![
                    game_round.season,
                    game_round.seq,
                    game.seq,
                    inning.seq,
                    inning.tb.to_string(),
                    inning.point
                ],
            )?;

            for count in inning.counts.iter() {
                conn.execute(
                    "INSERT OR REPLACE INTO count (
                        game_round_season, game_round_seq, game_seq, inning_seq, inning_tb, 
                        seq,
                        is_first_runner, is_second_runner, is_third_runner, batter_id, result, point, out
                        ) VALUES (
                         ?1, ?2, ?3, ?4, ?5,
                         ?6, 
                         ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        game_round.season,
                        game_round.seq,
                        game.seq,
                        inning.seq,
                        inning.tb.to_string(),
                        count.seq,
                        count.is_first_runner,
                        count.is_second_runner,
                        count.is_third_runner,
                        count.batter.id,
                        count.result.to_string(),
                        count.point,
                        count.out
                    ],
                )?;
            }
        }
    }

    conn.execute(
        "UPDATE game_season SET current_round_seq = current_round_seq + 1",
        params![],
    )?;

    Ok(())
}

fn load_games(game_round: &GameRound) -> Result<Vec<Game>> {
    // let mut games: Vec<Game> = game_round.games.clone();

    let conn = get_db_conn()?;
    let mut games: Vec<Game> = Vec::new();

    let mut stmt_game = conn.prepare(
        "SELECT 
                g.seq, 
                g.date,
                g.away_team_id, 
                t_away.name AS away_team_name,
                g.home_team_id, 
                t_home.name AS home_team_name,
                g.game_type
            FROM game g
            LEFT JOIN 
                Team t_away ON g.away_team_id = t_away.id
            LEFT JOIN 
                Team t_home ON g.home_team_id = t_home.id
            WHERE g.game_round_season = ?1 AND g.game_round_seq = ?2",
    )?;

    let games_iter = stmt_game.query_map([game_round.season, game_round.seq], |row| {
        Ok(Game {
            seq: row.get("seq")?,
            date: row.get("date")?,
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
            away_batters: Vec::new(),
            home_batters: Vec::new(),
        })
    })?;

    for game in games_iter {
        let mut _game = game?;
        let mut stmt_away_batter = conn.prepare(SELECT_BATTER_SQL)?;
        let away_batter_iter = stmt_away_batter.query_map([_game.away_team.id], |row| {
            Ok(Batter {
                id: row.get("id")?,
                name: row.get("name")?,
                mod_ba: row.get("mod_ba")?,
                mod_slg: row.get("mod_slg")?,
            })
        })?;

        let mut stmt_home_batter = conn.prepare(SELECT_BATTER_SQL)?;
        let home_batter_iter = stmt_home_batter.query_map([_game.home_team.id], |row| {
            Ok(Batter {
                id: row.get("id")?,
                name: row.get("name")?,
                mod_ba: row.get("mod_ba")?,
                mod_slg: row.get("mod_slg")?,
            })
        })?;

        for away_batter in away_batter_iter {
            _game.away_batters.push(away_batter?);
        }

        for home_batter in home_batter_iter {
            _game.home_batters.push(home_batter?);
        }

        let mut stmt_inning =
            conn.prepare("SELECT seq, tb, point 
                                FROM inning 
                                WHERE game_round_season = ?1 AND game_round_seq = ?2 AND game_seq = ?3
                                ORDER BY game_round_season ASC, game_round_seq ASC, game_seq ASC, seq ASC, tb DESC")?;
        let inning_iter =
            stmt_inning.query_map([game_round.season, game_round.seq, _game.seq], |row| {
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
                game_round_season = ?1 AND game_round_seq = ?2 AND game_seq = ?3 
                AND inning_seq = ?4 AND inning_tb = ?5",
        )?;
        for inning in inning_iter {
            let mut _inning = inning?;
            let count_iter = stmt_count.query_map(
                params![
                    game_round.season,
                    game_round.seq,
                    _game.seq,
                    _inning.seq,
                    _inning.tb.to_string()
                ],
                |row| {
                    Ok(Count {
                        seq: row.get("seq")?,
                        is_first_runner: row.get("is_first_runner")?,
                        is_second_runner: row.get("is_second_runner")?,
                        is_third_runner: row.get("is_third_runner")?,
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

            _game.innings.push(_inning);
        }
        games.push(_game.clone());
    }

    Ok(games)
}
