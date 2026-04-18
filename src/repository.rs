use super::shared::game::Count;
use super::shared::game::Game;
use super::shared::game::GameManager;
use super::shared::game::Inning;
use super::shared::player::Batter;
use super::shared::team::Team;
use super::shared::types::BattingResult;
use super::shared::types::InningType;
use anyhow::Result;
use directories::ProjectDirs;
// use rusqlite::Connection;
use rusqlite::types::{FromSql, FromSqlResult, ValueRef};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_rusqlite::{from_row, to_params_named};
use std::fs;
use std::sync::Arc;

pub const ERROR_LOAD_GAME_MANAGER: &str = "An error occurred in load_game_manager()";
pub const ERROR_LOAD_GAME: &str = "An error occurred in load_game()";
pub const ERROR_SAVE_GAME_MANAGER: &str = "An error occurred in save_game_manager()";
pub const ERROR_SAVE_GAME: &str = "An error occurred in save_game()";
const ERROR_NO_RECORD: &str = "No record found";
const ERROR_NO_DATA_DIR: &str = "The data directory is not found.";
const ERROR_PARSE: &str = "Parse error for";

const SELECT_BATTER_SQL: &str = "SELECT id, name, mod_ba, mod_slg FROM batter WHERE team_id = ?1";

struct SqlInningType(InningType);
impl FromSql for SqlInningType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let tb = value.as_str()?;

        tb.parse::<InningType>().map(SqlInningType).map_err(|e| {
            eprintln!("{} {}: {:?}", ERROR_PARSE, tb, e);
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
                eprintln!("{} {}: {:?}", ERROR_PARSE, br, e);
                rusqlite::types::FromSqlError::InvalidType
            })
    }
}

fn get_db_path() -> Result<std::path::PathBuf> {
    let proj_dirs = ProjectDirs::from("jp", "cosmi", "statbb").expect(ERROR_NO_DATA_DIR);

    let data_dir = proj_dirs.data_dir();
    fs::create_dir_all(data_dir)?;

    Ok(data_dir.join("statbb.db"))
}

pub fn load_game_manager() -> Result<GameManager> {
    let db_path = get_db_path()?;
    let conn = Connection::open(&db_path)?;

    let mut stmt = conn.prepare("SELECT season, phase FROM game_manager LIMIT 1")?;
    let mut rows = serde_rusqlite::from_rows::<GameManager>(stmt.query([])?);

    let game_manager = rows
        .next()
        .ok_or_else(|| anyhow::anyhow!(ERROR_NO_RECORD))??;

    Ok(game_manager)
}

pub fn save_game_manager(game_manager: &GameManager) -> Result<()> {
    let db_path = get_db_path()?;
    let conn = Connection::open(&db_path)?;
    conn.execute(
        "CSELECT id, name, mod_ba, mod_slg FROM batter WHERE team_id = ?1
        )",
        [],
    )?;

    let params = to_params_named(&game_manager)?;
    conn.execute(
        "INSERT OR REPLACE INTO game_manager (season, phase) VALUES (:season, :phase)",
        params.to_slice().as_slice(),
    )?;
    Ok(())
}

pub fn load_game(seq: i32) -> Result<Game> {
    let db_path = get_db_path()?;
    let conn = Connection::open(&db_path)?;

    let _team_ids: [i32; 2] = conn.query_row(
        "SELECT top_team_id, bottom_team_id FROM game WHERE seq = ?1",
        params![seq],
        |row| Ok([row.get("top_team_id")?, row.get("bottom_team_id")?]),
    )?;

    let top_team: Team = conn.query_row(
        "SELECT id, name FROM team WHERE id = ?1",
        params![_team_ids[0]],
        |row| {
            Ok(Team {
                id: row.get("id")?,
                name: row.get("name")?,
            })
        },
    )?;

    let bottom_team: Team = conn.query_row(
        "SELECT id, name FROM team WHERE id = ?1",
        params![_team_ids[1]],
        |row| {
            Ok(Team {
                id: row.get("id")?,
                name: row.get("name")?,
            })
        },
    )?;

    let mut stmt_top_batter = conn.prepare(SELECT_BATTER_SQL)?;
    let top_batter_iter = stmt_top_batter.query_map([top_team.id], |row| {
        Ok(Batter {
            id: row.get("id")?,
            name: row.get("name")?,
            mod_ba: row.get("mod_ba")?,
            mod_slg: row.get("mod_slg")?,
        })
    })?;

    let mut stmt_bottom_batter = conn.prepare(SELECT_BATTER_SQL)?;
    let bottom_batter_iter = stmt_bottom_batter.query_map([bottom_team.id], |row| {
        Ok(Batter {
            id: row.get("id")?,
            name: row.get("name")?,
            mod_ba: row.get("mod_ba")?,
            mod_slg: row.get("mod_slg")?,
        })
    })?;

    let mut top_batters: Vec<Batter> = Vec::new();
    for top_batter in top_batter_iter {
        top_batters.push(top_batter?);
    }

    let mut bottom_batters: Vec<Batter> = Vec::new();
    for bottom_batter in bottom_batter_iter {
        bottom_batters.push(bottom_batter?);
    }

    let mut stmt_inning = conn.prepare("SELECT tb, seq, point FROM inning WHERE game_seq = ?1")?;
    let inning_iter = stmt_inning.query_map([seq], |row| {
        Ok(Inning {
            tb: row.get::<_, SqlInningType>("tb")?.0,
            seq: row.get("seq")?,
            counts: Vec::new(),
            point: row.get("point")?,
        })
    })?;

    let mut innings: Vec<Inning> = Vec::new();
    let mut stmt_count = conn.prepare(
        "SELECT seq, is_first_runner, is_second_runner, is_third_runner, result, point, out, 
        id as batter_id, name as batter_name, mod_ba as batter_ba, mod_slg as batter_slg
        FROM count, batter 
        WHERE count.batter_id = batter.id AND inning_seq = ?1 AND inning_tb = ?2 AND game_seq = ?3",
    )?;
    for inning in inning_iter {
        let mut _inning = inning?;

        let count_iter =
            stmt_count.query_map(params![_inning.seq, _inning.tb.to_string(), seq], |row| {
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
            })?;

        for count in count_iter {
            _inning.counts.push(count?);
        }

        innings.push(_inning);
    }

    let mut _game: Game = Game {
        seq: seq,
        top_team: top_team,
        bottom_team: bottom_team,
        innings: innings,
        top_batters: top_batters,
        bottom_batters: bottom_batters,
    };

    Ok(_game)
}

pub fn save_game(game: &Game) -> Result<()> {
    let db_path = get_db_path()?;
    let conn = Connection::open(&db_path)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS game (
            seq INTEGER PRIMARY KEY,
            top_team_id INTEGER NOT NULL,
            bottom_team_id INTEGER NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "INSERT OR REPLACE INTO game (seq, top_team_id, bottom_team_id) VALUES (?1, ?2, ?3)",
        params![game.seq, game.top_team.id, game.bottom_team.id],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS inning (
            seq INTEGERY,
            game_seq INTEGER,
            tb TEXT,
            point INTEGER NOT NULL,
            PRIMARY KEY (seq, game_seq, tb)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS count (
            seq INTEGER,
            inning_seq INTEGER,
            inning_tb TEXT,
            game_seq INTEGER,
            is_first_runner BOOLEAN NOT NULL DEFAULT 0, 
            is_second_runner BOOLEAN NOT NULL DEFAULT 0, 
            is_third_runner BOOLEAN NOT NULL DEFAULT 0, 
            batter_id INTEGER,
            result TEXT NOT NULL,
            point INTEGER NOT NULL,
            out INTEGER NOT NULL,
            PRIMARY KEY (seq, inning_seq, inning_tb, game_seq)
        )",
        [],
    )?;

    for inning in game.innings.iter() {
        conn.execute(
            "INSERT OR REPLACE INTO inning (seq, game_seq, tb, point) VALUES (?1, ?2, ?3, ?4)",
            params![inning.seq, game.seq, inning.tb.to_string(), inning.point],
        )?;

        for count in inning.counts.iter() {
            conn.execute(
                "INSERT OR REPLACE INTO count (
                seq, 
                inning_seq, inning_tb, game_seq,  
                is_first_runner, is_second_runner, is_third_runner, batter_id, result, point, out) 
                VALUES (?1, 
                ?2, ?3, ?4, 
                ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    count.seq,
                    inning.seq,
                    inning.tb.to_string(),
                    game.seq,
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

    Ok(())
}

pub fn save_game_game_schedule(game_manager: GameManager) -> Result<()> {
    let db_path = get_db_path()?;
    let conn = Connection::open(&db_path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS game_manager (
            season INTEGER PRIMARY KEY,
            phase INTEGER NOT NULL
        )",
        [],
    )?;

    let params = to_params_named(&game_manager)?;
    conn.execute(
        "INSERT OR REPLACE INTO game_manager (season, phase) VALUES (:season, :phase)",
        params.to_slice().as_slice(),
    )?;
    Ok(())
}
