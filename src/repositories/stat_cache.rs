//! Derived, per-game additive statistics. Rates are calculated after summing counts.
use super::{batting_stat_cache, pitching_stat_cache};
use crate::repositories::db::DbClient;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::sync::Mutex;

// Dioxus resource cancellation does not stop an already-running blocking task.
// Serialize cold backfills across repositories so switching tabs cannot start
// another writer that times out while the first backfill is still running.
static CACHE_BUILD: Mutex<()> = Mutex::new(());
const CACHE_VERSION: u32 = 7;

pub fn ensure_schema(conn:&Connection)->Result<()> {
    // Version 5 counted each touched half-inning as a full inning. Replace only derived tables.
    let legacy_pitching: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('pitching_stat_totals') WHERE name='innings')",
        [],
        |row| row.get(0),
    )?;
    if legacy_pitching {
        conn.execute_batch("DROP TABLE IF EXISTS game_pitching_stat_cache; DROP TABLE pitching_stat_totals;")?;
    }
    conn.execute_batch(include_str!("../../migrations/upgrades/add_stat_cache.sql"))?;
    for table in ["game_batting_stat_cache", "batting_stat_totals"] {
        let mut schema = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = schema.query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for column in batting_stat_cache::COUNTS.split(',').filter(|column| {
            !columns.iter().any(|existing| existing == *column)
        }) {
            conn.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} INTEGER NOT NULL DEFAULT 0"
            ))?;
        }
    }
    super::team_stat_cache::ensure_schema(conn)?;
    Ok(())
}
fn refresh_totals(conn:&Connection,player:Option<i64>)->Result<()> {
    for (kind, columns) in [
        ("batting", batting_stat_cache::COUNTS),
        ("pitching", pitching_stat_cache::COUNTS),
    ] {
        let totals = format!("{kind}_stat_totals");
        let sums = columns.split(',').map(|name| format!("SUM({name})")).collect::<Vec<_>>().join(",");
        let filter = if player.is_some() { "player_id=?1" } else { "?1 IS NULL" };
        conn.execute(&format!("DELETE FROM {totals} WHERE {filter}"), [player])?;
        conn.execute(&format!(
            "INSERT INTO {totals}(player_id,{columns})
             SELECT player_id,{sums} FROM game_{kind}_stat_cache
             WHERE {filter} GROUP BY player_id"
        ), [player])?;
    }
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
    batting_stat_cache::fill(conn,Some(game))?;
    affected.extend(affected_players(conn,game)?);affected.sort_unstable();affected.dedup();
    for player in affected {refresh_totals(conn,Some(player))?;}
    super::team_stat_cache::refresh_game(conn,game)?;
    Ok(())
}
fn rebuild_in_transaction(conn:&Connection)->Result<()> {
    conn.execute_batch("DELETE FROM game_batting_stat_cache;DELETE FROM game_pitching_stat_cache;")?;
    batting_stat_cache::fill(conn,None)?;
    refresh_totals(conn,None)?;
    super::team_stat_cache::rebuild(conn)?;
    conn.execute("UPDATE stat_cache_state SET version=?1,ready=1 WHERE id=1",[CACHE_VERSION])?;Ok(())
}
pub(super) fn is_ready(conn: &Connection) -> rusqlite::Result<bool> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='stat_cache_state')",
        [], |row| row.get(0),
    )?;
    if !exists { return Ok(false); }
    Ok(conn.query_row("SELECT version=?1 AND ready=1 FROM stat_cache_state WHERE id=1", [CACHE_VERSION], |row| row.get(0))
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
    ensure_schema(conn)?;
    conn.execute_batch("DELETE FROM game_batting_stat_cache;DELETE FROM game_pitching_stat_cache;DELETE FROM batting_stat_totals;DELETE FROM pitching_stat_totals;")?;
    conn.execute("UPDATE stat_cache_state SET version=?1,ready=1 WHERE id=1", [CACHE_VERSION])?;
    Ok(())
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
            CREATE TABLE player_game_running(game_id INTEGER,count_seq INTEGER,seq INTEGER,event TEXT,ruling TEXT,
                target_runner_id INTEGER,runner_1st_id INTEGER,runner_2nd_id INTEGER,runner_3rd_id INTEGER);
            CREATE TABLE player_game_fielding(game_id INTEGER,count_seq INTEGER,error_type TEXT,interception_fielder_position TEXT);
            CREATE TABLE player_game_entry(game_id INTEGER,player_id INTEGER,position TEXT,start_count_seq INTEGER,end_count_seq INTEGER);
            CREATE TABLE player_info(id INTEGER,team_id INTEGER);
            CREATE TABLE player_game_batting(game_id INTEGER,count_seq INTEGER,batter_id INTEGER,pitcher_id INTEGER,result TEXT);
            CREATE TABLE player_game_pitching(game_id INTEGER,count_seq INTEGER,pitcher_id INTEGER,actual_location_x REAL DEFAULT 0,actual_location_y REAL DEFAULT 0);
            CREATE TABLE count(game_id INTEGER,seq INTEGER,inning_seq INTEGER,inning_tb TEXT,point INTEGER,out INTEGER DEFAULT 1);
            CREATE TABLE player_game_pitching_decision(game_id INTEGER,pitcher_id INTEGER,decision TEXT);
            INSERT INTO player_game_batting VALUES(1,1,1,10,'Single'),(1,2,1,10,'Strikeout'),(2,1,1,10,'Double');
            INSERT INTO player_game_pitching(game_id,count_seq,pitcher_id) VALUES(1,1,10),(1,2,10),(2,1,10);
            INSERT INTO count(game_id,seq,inning_seq,inning_tb,point) VALUES(1,1,1,'Top',1),(1,2,1,'Top',0),(2,1,1,'Top',2);
            INSERT INTO player_game_pitching_decision VALUES(1,10,'Win');").unwrap();
    }
    #[test]
    fn refresh_is_idempotent_and_replaces_contributions_in_totals() {
        let conn=fixture();refresh_game(&conn,1).unwrap();refresh_game(&conn,2).unwrap();refresh_game(&conn,1).unwrap();
        let ab:i64=conn.query_row("SELECT ab FROM batting_stat_totals WHERE player_id=1",[],|r|r.get(0)).unwrap();assert_eq!(ab,3);
        let p:(i64,i64,i64)=conn.query_row("SELECT games,outs,runs FROM pitching_stat_totals WHERE player_id=10",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();assert_eq!(p,(2,2,3));
        conn.execute("UPDATE player_game_batting SET result='HomeRun' WHERE game_id=1 AND count_seq=1",[]).unwrap();refresh_game(&conn,1).unwrap();
        let hits:(i64,i64)=conn.query_row("SELECT single,homerun FROM batting_stat_totals WHERE player_id=1",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();assert_eq!(hits,(0,1));
        conn.execute_batch("DELETE FROM player_game_batting WHERE game_id=2;DELETE FROM player_game_pitching WHERE game_id=2;").unwrap();refresh_game(&conn,2).unwrap();
        assert_eq!(conn.query_row("SELECT ab FROM batting_stat_totals WHERE player_id=1",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    }
    #[test]
    fn at_bats_exclude_nonterminal_pitches_walks_and_hit_by_pitch() {
        let conn=fixture();
        conn.execute_batch("INSERT INTO player_game_batting VALUES
            (1,3,1,10,'Take'),(1,4,1,10,'StrikeSwung'),(1,5,1,10,'Foul'),
            (1,6,1,10,'WildPitch'),(1,7,1,10,'Walk'),(1,8,1,10,'HitByPitch'),
            (1,9,1,10,'HomeRun'),(1,10,1,10,'Out'),(1,11,1,10,'FieldersChoice'),
            (1,12,1,10,'DoublePlay'),(1,13,1,10,'Triple');").unwrap();
        refresh_game(&conn,1).unwrap();
        let totals:(i64,i64)=conn.query_row("SELECT ab,homerun FROM batting_stat_totals WHERE player_id=1",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(totals,(7,1));
        // Rebuilding an old cache must correct the denominator, preserving HR counts.
        conn.execute_batch("UPDATE stat_cache_state SET version=3,ready=1;
            UPDATE game_batting_stat_cache SET ab=13 WHERE game_id=1;
            UPDATE batting_stat_totals SET ab=13;").unwrap();
        assert!(!is_ready(&conn).unwrap());
        rebuild_in_transaction(&conn).unwrap();
        assert!(is_ready(&conn).unwrap());
        assert_eq!(conn.query_row("SELECT ab,homerun FROM batting_stat_totals WHERE player_id=1",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?))).unwrap(),(8,1));
    }

    #[test]
    fn rebuilt_batting_totals_use_saved_plays_and_defensive_lineups() {
        let conn = fixture();
        conn.execute_batch(
            "DELETE FROM count;
             DELETE FROM player_game_batting;
             INSERT INTO count(game_id,seq,inning_seq,inning_tb,point)
             VALUES (1,1,1,'Top',0),(1,2,1,'Top',0),(1,3,1,'Top',1),(1,4,1,'Top',0),
                    (1,5,1,'Top',0),(1,6,1,'Top',0),(1,7,1,'Bottom',0),(1,8,1,'Bottom',0);
             INSERT INTO player_game_batting VALUES
                    (1,1,11,10,'Walk'),(1,2,12,10,'Double'),(1,3,13,10,'Out'),
                    (1,4,14,10,'Single'),(1,5,15,10,'HitByPitch'),(1,8,17,18,'Strikeout');
             INSERT INTO player_game_running VALUES
                    (1,2,1,'BaseSteal','Safe',11,NULL,11,NULL),
                    (1,2,2,'GrounderPlay','Safe',12,NULL,12,11),
                    (1,3,1,'TagUp','Safe',11,NULL,12,NULL),
                    (1,4,1,'GrounderPlay','Safe',14,14,12,NULL),
                    (1,6,1,'BaseSteal','Out',14,15,14,NULL);
             INSERT INTO player_info VALUES (16,2),(17,1);
             INSERT INTO player_game_entry VALUES (1,16,'CF',1,8),(1,17,'CF',1,8),(1,17,'LF',3,8);
             INSERT INTO player_game_fielding VALUES (1,3,'Fumble','CF');",
        ).unwrap();
        rebuild_in_transaction(&conn).unwrap();

        let counts = |player, columns| {
            conn.query_row(&format!("SELECT {columns} FROM batting_stat_totals WHERE player_id=?1"),
                [player], |row| (0..row.as_ref().column_count()).map(|i| row.get::<_, u32>(i)).collect::<rusqlite::Result<Vec<_>>>()).unwrap()
        };
        assert_eq!(counts(11, "games,pa,ab,bb,runs,sb"), [1, 1, 0, 1, 1, 1]);
        assert_eq!(counts(12, "double,risp_hits,risp_ab"), [1, 1, 1]);
        assert_eq!(counts(13, "pa,ab,sf,rbi,risp_ab"), [1, 0, 1, 1, 0]);
        assert_eq!(counts(14, "single,rbi,risp_hits,risp_ab,cs"), [1, 0, 1, 1, 1]);
        assert_eq!(counts(15, "pa,ab,hbp"), [1, 0, 1]);
        assert_eq!(counts(16, "games,pa,errors"), [1, 0, 1]);
        assert_eq!(counts(17, "games,ab,so,risp_ab,errors"), [1, 1, 1, 0, 0]);
    }

    #[test]
    fn version_four_batting_cache_is_backfilled_after_schema_upgrade() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        conn.execute_batch(
            "CREATE TABLE stat_cache_state(id INTEGER PRIMARY KEY,version INTEGER,ready INTEGER);
             INSERT INTO stat_cache_state VALUES(1,4,1);
             CREATE TABLE game_batting_stat_cache(game_id INTEGER,player_id INTEGER,ab INTEGER,
                 single INTEGER,double INTEGER,triple INTEGER,homerun INTEGER,rbi INTEGER,
                 PRIMARY KEY(game_id,player_id));
             CREATE TABLE batting_stat_totals(player_id INTEGER PRIMARY KEY,ab INTEGER,
                 single INTEGER,double INTEGER,triple INTEGER,homerun INTEGER,rbi INTEGER);
             INSERT INTO batting_stat_totals VALUES(1,99,0,0,0,0,0);",
        ).unwrap();
        assert!(!is_ready(&conn).unwrap());
        ensure_schema(&conn).unwrap();
        rebuild_in_transaction(&conn).unwrap();
        assert!(is_ready(&conn).unwrap());
        assert_eq!(
            conn.query_row("SELECT ab,pa,games FROM batting_stat_totals WHERE player_id=1", [],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, u32>(1)?, row.get::<_, u32>(2)?))).unwrap(),
            (3, 3, 2),
        );
    }

    #[test]
    fn version_five_pitching_cache_rebuilds_outs_and_runner_responsibility() {
        let conn = fixture();
        conn.execute_batch(
            "DROP TABLE game_pitching_stat_cache;
             DROP TABLE pitching_stat_totals;
             CREATE TABLE game_pitching_stat_cache(game_id INTEGER,player_id INTEGER,innings INTEGER);
             CREATE TABLE pitching_stat_totals(player_id INTEGER PRIMARY KEY,innings INTEGER);
             INSERT INTO pitching_stat_totals VALUES(10,99);
             UPDATE stat_cache_state SET version=5,ready=1;
             DELETE FROM count WHERE game_id=1;
             DELETE FROM player_game_batting WHERE game_id=1;
             DELETE FROM player_game_pitching WHERE game_id=1;
             WITH RECURSIVE inning(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM inning WHERE n<6)
             INSERT INTO count VALUES
                (1,7,7,'Top',0,1),(1,8,7,'Top',0,1),(1,9,7,'Top',2,1),
                (1,10,7,'Top',0,1),(1,11,7,'Top',0,1),(1,12,7,'Top',0,2),
                (1,13,1,'Bottom',0,3)
             UNION ALL SELECT 1,n,n,'Top',0,3 FROM inning;
             INSERT INTO player_game_pitching(game_id,count_seq,pitcher_id)
                SELECT game_id,seq,CASE WHEN seq<9 THEN 10 WHEN seq<13 THEN 20 ELSE 30 END
                FROM count WHERE game_id=1;
             UPDATE player_game_pitching SET actual_location_x=4 WHERE game_id=1 AND count_seq=11;
             INSERT INTO player_game_batting VALUES
                (1,8,11,10,'Walk'),(1,9,12,20,'HomeRun'),(1,10,13,20,'HitByPitch'),
                (1,11,14,20,'Walk'),(1,12,15,20,'Strikeout');
             DELETE FROM player_game_pitching_decision WHERE game_id=1;
             INSERT INTO player_game_pitching_decision VALUES(1,20,'Win');",
        ).unwrap();
        assert!(!is_ready(&conn).unwrap());
        ensure_schema(&conn).unwrap();
        rebuild_in_transaction(&conn).unwrap();

        let counts = |player, columns| {
            conn.query_row(
                &format!("SELECT {columns} FROM game_pitching_stat_cache WHERE game_id=1 AND player_id=?1"),
                [player],
                |row| (0..row.as_ref().column_count()).map(|i| row.get::<_, u32>(i)).collect::<rusqlite::Result<Vec<_>>>(),
            ).unwrap()
        };
        assert_eq!(counts(10, "starts,outs,complete_games,quality_starts,bb,runs,earned_runs"), [1, 19, 0, 1, 1, 1, 1]);
        assert_eq!(counts(20, "starts,outs,relief_wins,hits,home_runs,so,bb,hbp,wild_pitches,runs"), [0, 1, 1, 1, 1, 1, 1, 1, 0, 1]);
        assert_eq!(counts(30, "starts,complete_games,shutouts"), [1, 1, 1]);
        assert!(is_ready(&conn).unwrap());

        conn.execute("INSERT INTO player_game_fielding VALUES(1,7,'Fumble','CF')", []).unwrap();
        // Classify each pitcher's run as uncertain, including the inherited runner.
        // The starter still has a QS because his maximum possible earned runs is one.
        refresh_game(&conn, 1).unwrap();
        assert_eq!(counts(10, "earned_runs,unclassified_runs,quality_starts"), [0, 1, 1]);
        assert_eq!(counts(20, "earned_runs,unclassified_runs"), [0, 1]);
    }

    #[test]
    fn version_six_wild_pitch_totals_rebuild_using_runner_advancement() {
        let conn = fixture();
        conn.execute_batch(
            "DELETE FROM count;
             DELETE FROM player_game_batting;
             DELETE FROM player_game_pitching;
             INSERT INTO count VALUES
                (1,1,1,'Top',0,0),(1,2,1,'Top',0,0),(1,3,1,'Top',0,0),
                (1,4,2,'Top',0,0),(1,5,2,'Top',0,0),
                (1,6,3,'Top',0,0),(1,7,3,'Top',0,0),(1,8,3,'Top',0,0),
                (1,9,4,'Top',0,0),(1,10,4,'Top',0,0);
             INSERT INTO player_game_batting VALUES
                (1,1,11,10,'WildPitch'),(1,2,11,10,'Walk'),(1,3,12,10,'WildPitch'),
                (1,4,13,10,'Walk'),(1,5,14,10,'Walk'),
                (1,6,15,10,'Walk'),(1,7,16,10,'WildPitch'),(1,8,16,10,'Walk'),
                (1,9,17,10,'Walk'),(1,10,18,10,'Strikeout');
             INSERT INTO player_game_pitching(game_id,count_seq,pitcher_id,actual_location_y)
                SELECT 1,seq,10,CASE WHEN seq IN (1,3,5,7,8,9) THEN -2.4 ELSE 0 END
                FROM count;",
        )
        .unwrap();
        refresh_game(&conn, 1).unwrap();
        let counts = || {
            conn.query_row(
                "SELECT wild_pitches,bb,runs,so FROM pitching_stat_totals WHERE player_id=10",
                [],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?, row.get::<_, u32>(1)?,
                        row.get::<_, u32>(2)?, row.get::<_, u32>(3)?,
                    ))
                },
            )
            .unwrap()
        };
        // 3 and 7 advance a runner; ball four at 8 advances a non-forced runner.
        assert_eq!(counts(), (3, 6, 0, 1));

        conn.execute_batch(
            "UPDATE stat_cache_state SET version=6,ready=1;
             UPDATE game_pitching_stat_cache SET wild_pitches=6;
             UPDATE pitching_stat_totals SET wild_pitches=6;",
        )
        .unwrap();
        assert!(!is_ready(&conn).unwrap());
        rebuild_in_transaction(&conn).unwrap();
        assert!(is_ready(&conn).unwrap());
        assert_eq!(counts(), (3, 6, 0, 1));
    }

    #[test]
    fn blocked_ball_four_is_rebuilt_as_an_ordinary_walk() {
        let conn = fixture();
        super::batting_stat_cache::ensure_catcher_pitch_columns(&conn).unwrap();
        super::batting_stat_cache::ensure_catcher_pitch_columns(&conn).unwrap();
        conn.execute_batch(
            "DELETE FROM count;
             DELETE FROM player_game_batting;
             DELETE FROM player_game_pitching;
             INSERT INTO count VALUES
                (1,1,1,'Top',0,0),(1,2,1,'Top',0,0),(1,3,1,'Top',0,0);
             INSERT INTO player_game_batting(game_id,count_seq,batter_id,pitcher_id,result,catcher_blocked,catcher_passed_ball) VALUES
                (1,1,11,10,'Double',NULL,NULL),(1,2,12,10,'Walk',1,NULL),
                (1,3,13,10,'WildPitch',0,1);
             INSERT INTO player_game_pitching(game_id,count_seq,pitcher_id,actual_location_y)
                VALUES (1,1,10,0),(1,2,10,-2.5),(1,3,10,-2.5);",
        ).unwrap();
        rebuild_in_transaction(&conn).unwrap();
        let totals = conn.query_row(
            "SELECT bb,wild_pitches,runs FROM pitching_stat_totals WHERE player_id=10",
            [],
            |r| Ok((r.get::<_, u32>(0)?, r.get::<_, u32>(1)?, r.get::<_, u32>(2)?)),
        ).unwrap();
        assert_eq!(totals, (1, 0, 0));
        conn.execute("UPDATE player_game_batting SET catcher_passed_ball=NULL WHERE count_seq=3", []).unwrap();
        rebuild_in_transaction(&conn).unwrap();
        assert_eq!(conn.query_row(
            "SELECT wild_pitches FROM pitching_stat_totals WHERE player_id=10",
            [], |r| r.get::<_, u32>(0),
        ).unwrap(), 1);
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
