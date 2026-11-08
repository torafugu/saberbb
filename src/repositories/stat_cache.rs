//! Derived, per-game additive statistics. Rates are calculated after summing counts.
use crate::repositories::db::DbClient;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::sync::Mutex;

// Dioxus resource cancellation does not stop an already-running blocking task.
// Serialize cold backfills across repositories so switching tabs cannot start
// another writer that times out while the first backfill is still running.
static CACHE_BUILD: Mutex<()> = Mutex::new(());

pub fn ensure_schema(conn:&Connection)->Result<()> {
    conn.execute_batch(include_str!("../../migrations/upgrades/add_stat_cache.sql"))?;
    super::team_stat_cache::ensure_schema(conn)?;Ok(())
}
// Separate keyed SQL avoids an OR predicate that would force a full log scan per game.
fn scope(sql:&str,keyed:bool)->String {
    let mut sql=sql.to_owned();
    for column in ["b.game_id","game_id","player_id"] {
        let predicate=format!("?1 IS NULL OR {column}=?1");
        sql=sql.replace(&predicate,&if keyed{format!("{column}=?1")}else{"?1 IS NULL".into()});
    }
    sql
}
fn fill(conn:&Connection,game:Option<u32>)->Result<()> {
    conn.execute(&scope("INSERT INTO game_batting_stat_cache
         SELECT b.game_id,b.batter_id,COUNT(*),
             SUM(b.result='Single'),SUM(b.result='Double'),SUM(b.result='Triple'),SUM(b.result='HomeRun'),
             COALESCE(SUM(c.point),0)
         FROM player_game_batting b LEFT JOIN count c ON c.game_id=b.game_id AND c.seq=b.count_seq
         WHERE (?1 IS NULL OR b.game_id=?1) GROUP BY b.game_id,b.batter_id",game.is_some()),[game])?;
    conn.execute(&scope("WITH p AS (SELECT * FROM player_game_pitching WHERE ?1 IS NULL OR game_id=?1),
         b AS (SELECT * FROM player_game_batting WHERE ?1 IS NULL OR game_id=?1),
         d AS (SELECT * FROM player_game_pitching_decision WHERE ?1 IS NULL OR game_id=?1),
         ids AS (SELECT game_id,pitcher_id FROM p UNION SELECT game_id,pitcher_id FROM b UNION SELECT game_id,pitcher_id FROM d),
         pitch AS (
             SELECT p.game_id,p.pitcher_id,1 AS games,
                 COUNT(DISTINCT CASE WHEN c.inning_seq IS NOT NULL AND c.inning_tb IS NOT NULL
                     THEN p.game_id||'-'||c.inning_seq||'-'||c.inning_tb ELSE p.game_id||'-'||p.count_seq END) AS innings,
                 COALESCE(SUM(c.point),0) AS runs
             FROM p LEFT JOIN count c ON c.game_id=p.game_id AND c.seq=p.count_seq GROUP BY p.game_id,p.pitcher_id
         ), result AS (SELECT game_id,pitcher_id,SUM(result='Strikeout') AS so,SUM(result IN ('Walk','HitByPitch')) AS bb FROM b GROUP BY game_id,pitcher_id),
         decision AS (SELECT game_id,pitcher_id,SUM(decision='Win') AS wins,SUM(decision='Loss') AS losses,
             SUM(decision='Save') AS saves,SUM(decision='Hold') AS holds FROM d GROUP BY game_id,pitcher_id)
         INSERT INTO game_pitching_stat_cache
         SELECT ids.game_id,ids.pitcher_id,COALESCE(pitch.games,0),COALESCE(pitch.innings,0),COALESCE(pitch.runs,0),
             COALESCE(decision.wins,0),COALESCE(decision.losses,0),COALESCE(decision.saves,0),COALESCE(decision.holds,0),
             COALESCE(result.so,0),COALESCE(result.bb,0)
         FROM ids LEFT JOIN pitch USING(game_id,pitcher_id) LEFT JOIN result USING(game_id,pitcher_id)
         LEFT JOIN decision USING(game_id,pitcher_id)",game.is_some()),[game])?;
    Ok(())
}

fn refresh_totals(conn:&Connection,player:Option<i64>)->Result<()> {
    conn.execute(&scope("DELETE FROM batting_stat_totals WHERE ?1 IS NULL OR player_id=?1",player.is_some()),[player])?;
    conn.execute(&scope("INSERT INTO batting_stat_totals SELECT player_id,SUM(ab),SUM(single),SUM(double),SUM(triple),SUM(homerun),SUM(rbi) FROM game_batting_stat_cache WHERE ?1 IS NULL OR player_id=?1 GROUP BY player_id",player.is_some()),[player])?;
    conn.execute(&scope("DELETE FROM pitching_stat_totals WHERE ?1 IS NULL OR player_id=?1",player.is_some()),[player])?;
    conn.execute(&scope("INSERT INTO pitching_stat_totals SELECT player_id,SUM(games),SUM(innings),SUM(runs),SUM(wins),SUM(losses),SUM(saves),SUM(holds),SUM(so),SUM(bb) FROM game_pitching_stat_cache WHERE ?1 IS NULL OR player_id=?1 GROUP BY player_id",player.is_some()),[player])?;
    Ok(())
}
fn affected_players(conn:&Connection,game:u32)->Result<Vec<i64>> {
    let mut stmt=conn.prepare("SELECT player_id FROM game_batting_stat_cache WHERE game_id=?1 UNION SELECT player_id FROM game_pitching_stat_cache WHERE game_id=?1")?;
    Ok(stmt.query_map([game],|r|r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
}
/// Called inside the game-result transaction, after all logs and pitching decisions.
pub fn refresh_game(conn:&Connection,game:u32)->Result<()> {
    ensure_schema(conn)?;
    let mut affected=affected_players(conn,game)?;
    conn.execute("DELETE FROM game_batting_stat_cache WHERE game_id=?1",[game])?;
    conn.execute("DELETE FROM game_pitching_stat_cache WHERE game_id=?1",[game])?;
    fill(conn,Some(game))?;
    affected.extend(affected_players(conn,game)?);affected.sort_unstable();affected.dedup();
    for player in affected {refresh_totals(conn,Some(player))?;}
    super::team_stat_cache::refresh_game(conn,game)?;
    Ok(())
}
fn rebuild_in_transaction(conn:&Connection)->Result<()> {
    conn.execute_batch("DELETE FROM game_batting_stat_cache;DELETE FROM game_pitching_stat_cache;")?;
    fill(conn,None)?;
    refresh_totals(conn,None)?;
    super::team_stat_cache::rebuild(conn)?;
    conn.execute("UPDATE stat_cache_state SET version=3,ready=1 WHERE id=1",[])?;Ok(())
}
fn is_ready(conn: &Connection) -> rusqlite::Result<bool> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='stat_cache_state')",
        [], |row| row.get(0),
    )?;
    if !exists { return Ok(false); }
    Ok(conn.query_row("SELECT version=3 AND ready=1 FROM stat_cache_state WHERE id=1", [], |row| row.get(0))
        .optional()?.unwrap_or(false))
}

pub fn ensure_ready(db: &DbClient) -> Result<()> {
    // Warm displays remain read-only and do not wait for the initialization gate.
    {
        let conn = db.get_conn()?;
        if is_ready(&conn)? { return Ok(()); }
    }
    let _build = CACHE_BUILD.lock().unwrap_or_else(|p| p.into_inner());
    let mut conn = db.get_conn()?;
    // The worker ahead of us may have finished while we waited, even if its UI
    // resource was cancelled. Recheck before asking SQLite for a write lock.
    if is_ready(&conn)? { return Ok(()); }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_schema(&tx)?;
    if !is_ready(&tx)? { rebuild_in_transaction(&tx)?; }
    tx.commit()?;
    Ok(())
}
pub fn rebuild(db: &DbClient) -> Result<()> {
    let _build = CACHE_BUILD.lock().unwrap_or_else(|p| p.into_inner());
    let mut conn = db.get_conn()?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_schema(&tx)?;
    rebuild_in_transaction(&tx)?;
    tx.commit()?;
    Ok(())
}
pub fn clear(conn:&Connection)->Result<()> {
    super::team_stat_cache::clear(conn)?;
    ensure_schema(conn)?;conn.execute_batch("DELETE FROM game_batting_stat_cache;DELETE FROM game_pitching_stat_cache;DELETE FROM batting_stat_totals;DELETE FROM pitching_stat_totals;UPDATE stat_cache_state SET version=3,ready=1 WHERE id=1;")?;Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture()->Connection {
        let conn=Connection::open_in_memory().unwrap();
        seed(&conn);
        ensure_schema(&conn).unwrap();conn
    }
    fn seed(conn: &Connection) {
        conn.execute_batch("CREATE TABLE game(id INTEGER PRIMARY KEY,away_team_id INTEGER,home_team_id INTEGER,actual_date TEXT,away_points INTEGER,home_points INTEGER);
            INSERT INTO game VALUES(1,1,2,'2026-04-01',1,0),(2,1,2,'2026-04-02',2,0);
            CREATE TABLE player_game_running(game_id INTEGER,count_seq INTEGER,event TEXT,ruling TEXT);
            CREATE TABLE player_game_fielding(game_id INTEGER,count_seq INTEGER,error_type TEXT);
            CREATE TABLE player_game_batting(game_id INTEGER,count_seq INTEGER,batter_id INTEGER,pitcher_id INTEGER,result TEXT);
            CREATE TABLE player_game_pitching(game_id INTEGER,count_seq INTEGER,pitcher_id INTEGER);
            CREATE TABLE count(game_id INTEGER,seq INTEGER,inning_seq INTEGER,inning_tb TEXT,point INTEGER,out INTEGER DEFAULT 1);
            CREATE TABLE player_game_pitching_decision(game_id INTEGER,pitcher_id INTEGER,decision TEXT);
            INSERT INTO player_game_batting VALUES(1,1,1,10,'Single'),(1,2,1,10,'Strikeout'),(2,1,1,10,'Double');
            INSERT INTO player_game_pitching VALUES(1,1,10),(1,2,10),(2,1,10);
            INSERT INTO count(game_id,seq,inning_seq,inning_tb,point) VALUES(1,1,1,'Top',1),(1,2,1,'Top',0),(2,1,1,'Top',2);
            INSERT INTO player_game_pitching_decision VALUES(1,10,'Win');").unwrap();
    }
    #[test]
    fn refresh_is_idempotent_and_replaces_contributions_in_totals() {
        let conn=fixture();refresh_game(&conn,1).unwrap();refresh_game(&conn,2).unwrap();refresh_game(&conn,1).unwrap();
        let ab:i64=conn.query_row("SELECT ab FROM batting_stat_totals WHERE player_id=1",[],|r|r.get(0)).unwrap();assert_eq!(ab,3);
        let p:(i64,i64,i64)=conn.query_row("SELECT games,innings,runs FROM pitching_stat_totals WHERE player_id=10",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();assert_eq!(p,(2,2,3));
        conn.execute("UPDATE player_game_batting SET result='HomeRun' WHERE game_id=1 AND count_seq=1",[]).unwrap();refresh_game(&conn,1).unwrap();
        let hits:(i64,i64)=conn.query_row("SELECT single,homerun FROM batting_stat_totals WHERE player_id=1",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();assert_eq!(hits,(0,1));
        conn.execute_batch("DELETE FROM player_game_batting WHERE game_id=2;DELETE FROM player_game_pitching WHERE game_id=2;").unwrap();refresh_game(&conn,2).unwrap();
        assert_eq!(conn.query_row("SELECT ab FROM batting_stat_totals WHERE player_id=1",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    }
    #[test]
    fn failed_cache_update_rolls_back_logs_and_previous_cache() {
        let mut conn=fixture();refresh_game(&conn,1).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_cache BEFORE INSERT ON game_pitching_stat_cache BEGIN SELECT RAISE(ABORT,'test rejection');END;").unwrap();
        {
            let tx=conn.transaction().unwrap();
            tx.execute("INSERT INTO player_game_batting VALUES(1,3,1,10,'HomeRun')",[]).unwrap();
            assert!(refresh_game(&tx,1).is_err());
        }
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM player_game_batting WHERE game_id=1",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        assert_eq!(conn.query_row("SELECT ab FROM batting_stat_totals WHERE player_id=1",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM game_pitching_stat_cache",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    }
    #[test]
    fn backfill_and_reset_cover_all_derived_tables() {
        let conn=fixture();rebuild_in_transaction(&conn).unwrap();
        assert_eq!(conn.query_row("SELECT ab FROM batting_stat_totals",[],|r|r.get::<_,i64>(0)).unwrap(),3);
        clear(&conn).unwrap();
        for table in ["game_batting_stat_cache","game_pitching_stat_cache","batting_stat_totals","pitching_stat_totals","game_team_stat_cache","team_stat_totals"] {
            assert_eq!(conn.query_row(&format!("SELECT COUNT(*) FROM {table}"),[],|r|r.get::<_,i64>(0)).unwrap(),0);
        }
    }
    fn file_db() -> (DbClient, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("saberbb-cache-lock-{}-{}.db", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let pool = deadpool::managed::Pool::builder(crate::repositories::db::SqliteManager::from_path(path.clone()))
            .max_size(8).build().unwrap();
        let db = DbClient { db: crate::repositories::db::SqlDb::from_pool(pool) };
        let conn = db.get_conn().unwrap();
        assert_eq!(conn.query_row("PRAGMA busy_timeout", [], |r| r.get::<_, i64>(0)).unwrap(), 30_000);
        seed(&conn);
        drop(conn);
        (db, path)
    }
    #[test]
    fn existing_logs_backfill_once_under_concurrent_requests_without_new_games() {
        let (db, path) = file_db();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8).map(|_| {
            let db = db.clone(); let barrier = barrier.clone();
            std::thread::spawn(move || { barrier.wait(); ensure_ready(&db).unwrap(); })
        }).collect();
        for worker in workers { worker.join().unwrap(); }
        let conn = db.get_conn().unwrap();
        assert!(is_ready(&conn).unwrap());
        assert_eq!(conn.query_row("SELECT ab FROM batting_stat_totals WHERE player_id=1", [], |r| r.get::<_,i64>(0)).unwrap(), 3);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM game_pitching_stat_cache", [], |r| r.get::<_,i64>(0)).unwrap(), 2);
        drop(conn); drop(db); std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn cold_request_waits_for_active_backfill_instead_of_sqlite_timeout() {
        let (db, path) = file_db();
        // Emulate a long-running first initializer. A following resource must wait
        // at the process gate rather than time out trying BEGIN IMMEDIATE again.
        let build = CACHE_BUILD.lock().unwrap_or_else(|p| p.into_inner());
        let mut conn = db.get_conn().unwrap();
        ensure_schema(&conn).unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let waiting_db = db.clone();
        let worker = std::thread::spawn(move || {
            waiting_db.get_conn().unwrap().busy_timeout(std::time::Duration::from_millis(10)).unwrap();
            send.send(ensure_ready(&waiting_db)).unwrap();
        });
        let premature = receive.recv_timeout(std::time::Duration::from_millis(150));
        rebuild_in_transaction(&tx).unwrap(); tx.commit().unwrap();
        drop(build);
        assert!(matches!(premature, Err(std::sync::mpsc::RecvTimeoutError::Timeout)), "initializer must wait for the current builder");
        receive.recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap();
        worker.join().unwrap();
        drop(conn); drop(db); std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn no_played_games_returns_empty_ready_cache() {
        let (db, path) = file_db();
        db.get_conn().unwrap().execute_batch("DELETE FROM player_game_batting; DELETE FROM player_game_pitching; DELETE FROM player_game_pitching_decision; DELETE FROM count; DELETE FROM game;").unwrap();
        ensure_ready(&db).unwrap();
        let conn = db.get_conn().unwrap();
        assert!(is_ready(&conn).unwrap());
        for table in ["batting_stat_totals", "pitching_stat_totals"] {
            assert_eq!(conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_,i64>(0)).unwrap(), 0);
        }
        drop(conn); drop(db); std::fs::remove_file(path).unwrap();
    }

}
