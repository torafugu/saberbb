use crate::config::load_app_config;
use crate::error::AppError;
use anyhow::{Context, Result};
use deadpool::managed::{Manager, Metrics, Object, Pool, RecycleResult};
use rusqlite::Connection;
use rusqlite::types::FromSql;
use std::fs;
use std::path::Path;
use tracing::info;

#[derive(Clone, Debug)]
pub struct SqliteManager {
    path: std::path::PathBuf,
    read_only: bool,
}

impl SqliteManager {
    #[cfg(test)]
    pub(crate) fn from_path(path: std::path::PathBuf) -> Self {
        Self { path, read_only: false }
    }
}

impl Manager for SqliteManager {
    type Type = Connection;
    type Error = rusqlite::Error;

    async fn create(&self) -> Result<Connection, rusqlite::Error> {
        let conn = if self.read_only {
            Connection::open_with_flags(&self.path,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?
        } else { Connection::open(&self.path)? };
        // A game save or a separate process may briefly own SQLite's writer lock.
        // Set this explicitly on every pooled connection (SQLite defaults vary).
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        Ok(conn)
    }

    async fn recycle(
        &self,
        _conn: &mut Connection,
        _metrics: &Metrics,
    ) -> RecycleResult<rusqlite::Error> {
        Ok(()) // SQLite does not need recycle.
    }
}

pub type SqlitePool = Pool<SqliteManager, Object<SqliteManager>>;

#[derive(thiserror::Error, Debug)]
pub enum DbError {
    #[error("DB Pool error : {0}")]
    Pool(#[from] Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Clone, Debug)]
pub struct SqlDb {
    pool: SqlitePool,
}

impl SqlDb {
    pub fn new() -> Result<Self> {
        let pool = create_sqlite_pool()?;
        Ok(Self { pool })
    }

    /// Open existing data for diagnostics without creating or modifying the source DB.
    pub fn read_only(path: &Path) -> Result<Self> {
        let manager=SqliteManager {path:path.to_path_buf(),read_only:true};
        let pool=Pool::builder(manager).max_size(8).build()?;
        Ok(Self {pool})
    }

    pub fn from_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn get_conn(&self) -> Result<Object<SqliteManager>, DbError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            let pool = self.pool.clone();
            std::thread::spawn(move || futures::executor::block_on(pool.get()))
                .join()
                .map_err(|_| {
                    DbError::Pool(Box::new(std::io::Error::other(
                        "DB connection thread panicked",
                    )))
                })?
                .map_err(|e| Box::new(e) as _)
                .map_err(DbError::Pool)
        } else {
            futures::executor::block_on(self.pool.get())
                .map_err(|e| Box::new(e) as _)
                .map_err(DbError::Pool)
        }
    }
}

pub fn create_sqlite_pool() -> Result<Pool<SqliteManager>> {
    let db_path = load_app_config()?.database_path;
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let manager = SqliteManager { path: db_path, read_only: false };

    Pool::builder(manager)
        .max_size(8)
        .build()
        .map_err(|e| anyhow::anyhow!("Faile to build DB pool: {}", e))
}

pub trait FromRow: Sized {
    type Error;
    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error>
    where
        Self: Sized;
}

impl<T: FromSql> FromRow for T {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let value: T = row.get(0).map_err(|e| AppError::Database(e))?;
        Ok(value)
    }
}

pub trait FromRowWithCtx<Ctx>: Sized {
    type Error;
    fn from_row_with_ctx(row: &rusqlite::Row, ctx: &Ctx) -> Result<Self, Self::Error>;
}

#[derive(Clone, Debug)]
pub struct DbClient {
    pub db: SqlDb,
}

impl DbClient {
    pub fn new() -> Result<Self> {
        let db = SqlDb::new()?;
        Ok(Self { db })
    }

    pub fn get_conn(&self) -> Result<Object<SqliteManager>, DbError> {
        self.db.get_conn()
    }

    pub fn execute(&self, query: &str, params: &[&dyn rusqlite::ToSql]) -> Result<usize, AppError> {
        let conn = self.get_conn()?;
        conn.execute(query, params)
            .map_err(|e| AppError::Database(e))
    }

    pub fn execute_tx(
        &self,
        tx: &rusqlite::Transaction,
        query: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<usize, AppError> {
        tx.execute(query, params).map_err(|e| AppError::Database(e))
    }

    pub fn execute_insert_tx(
        &self,
        tx: &rusqlite::Transaction,
        query: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<i64, AppError> {
        tx.execute(query, params)
            .map_err(|e| AppError::Database(e))?;

        Ok(tx.last_insert_rowid())
    }

    pub fn transaction<F, R>(&self, f: F) -> Result<R, AppError>
    where
        F: FnOnce(&rusqlite::Transaction) -> Result<R, AppError>,
    {
        let mut conn = self.get_conn()?;
        let tx = conn.transaction().map_err(|e| AppError::Database(e))?;

        match f(&tx) {
            Ok(result) => {
                tx.commit().map_err(|e| AppError::Database(e))?;
                Ok(result)
            }
            Err(err) => Err(err),
        }
    }

    pub fn query_row<T>(&self, query: &str, params: &[&dyn rusqlite::ToSql]) -> Result<T, AppError>
    where
        T: FromRow<Error = AppError>,
    {
        let conn = self.get_conn()?;
        let mut stmt = conn.prepare(query).map_err(|e| AppError::Database(e))?;

        let mut mapped_rows = stmt
            .query_map(params, |row| Ok(T::from_row(row)))
            .map_err(|e| AppError::Database(e))?;

        let first_row_res = mapped_rows
            .next()
            .ok_or_else(|| AppError::NotFound(format!("No row found for query: {}", query)))?;

        let from_row_res = first_row_res.map_err(|e| AppError::Database(e))?;

        let item = from_row_res?;
        Ok(item)
    }

    pub fn query_row_with_ctx<T, Ctx>(
        &self,
        query: &str,
        params: &[&dyn rusqlite::ToSql],
        ctx: &Ctx,
    ) -> Result<T, AppError>
    where
        T: FromRowWithCtx<Ctx, Error = AppError>,
    {
        let conn = self.get_conn()?;
        let mut stmt = conn.prepare(query).map_err(|e| AppError::Database(e))?;

        let mut mapped_rows = stmt
            .query_map(params, |row| Ok(T::from_row_with_ctx(row, ctx)))
            .map_err(|e| AppError::Database(e))?;

        let first_row_res = mapped_rows
            .next()
            .ok_or_else(|| AppError::NotFound(format!("No row found for query: {}", query)))?;

        let from_row_res = first_row_res.map_err(|e| AppError::Database(e))?;

        let item = from_row_res?;
        Ok(item)
    }

    pub fn query_rows<T>(
        &self,
        query: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<T>, AppError>
    where
        T: FromRow<Error = AppError>,
    {
        let conn = self.get_conn()?;
        let mut stmt = conn.prepare(query).map_err(|e| AppError::Database(e))?;

        let mapped_rows = stmt
            .query_map(params, |row| Ok(T::from_row(row)))
            .map_err(|e| AppError::Database(e))?;

        let mut result = Vec::new();
        for row_res in mapped_rows {
            let from_row_res = row_res.map_err(|e| AppError::Database(e))?;
            let item = from_row_res?;
            result.push(item);
        }

        Ok(result)
    }

    pub fn query_rows_with_ctx<T, Ctx>(
        &self,
        query: &str,
        params: &[&dyn rusqlite::ToSql],
        ctx: &Ctx,
    ) -> Result<Vec<T>, AppError>
    where
        T: FromRowWithCtx<Ctx, Error = AppError>,
    {
        let conn = self.get_conn()?;
        let mut stmt = conn.prepare(query).map_err(|e| AppError::Database(e))?;

        let mapped_rows = stmt
            .query_map(params, |row| Ok(T::from_row_with_ctx(row, ctx)))
            .map_err(|e| AppError::Database(e))?;

        let mut result = Vec::new();
        for row_res in mapped_rows {
            let from_row_res = row_res.map_err(|e| AppError::Database(e))?;
            let item = from_row_res?;
            result.push(item);
        }

        Ok(result)
    }
}

/// Run the schema and seed scripts and import name CSVs into the configured database.
pub fn setup() -> Result<()> {
    let mut conn = SqlDb::new()?.get_conn()?;
    setup_database(&mut conn, Path::new("migrations"))
}

fn setup_database(conn: &mut Connection, migrations_dir: &Path) -> Result<()> {
    let tx = conn.transaction()?;
    for directory in ["ddl", "dml"] {
        let directory = migrations_dir.join(directory);
        let mut scripts = Vec::new();
        for entry in fs::read_dir(&directory)
            .with_context(|| format!("Failed to read {}", directory.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_file() && path.extension().is_some_and(|ext| ext == "sql") {
                scripts.push(path);
            }
        }
        scripts.sort();

        for path in scripts {
            let sql = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            info!(script = %path.display(), "Executing setup SQL");
            tx.execute_batch(&sql)
                .with_context(|| format!("Failed to execute {}", path.display()))?;
        }
    }
    // These DDLs are deliberately additive for upgrades. Unlike the game/log
    // DDLs, rerunning them does not clear old rows when --setup reuses game IDs.
    for table in ["player_game_fielding_candidate", "player_game_contact_state"] {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table], |row| row.get(0),
        )?;
        if exists { tx.execute(&format!("DELETE FROM {table}"), [])?; }
    }
    for (table, columns) in [
        (
            "first_names",
            &["id", "name", "reading", "gender", "country"][..],
        ),
        ("last_names", &["id", "name", "reading", "country"][..]),
    ] {
        let path = migrations_dir.join("data").join(format!("{table}.csv"));
        import_names(&tx, &path, table, columns)
            .with_context(|| format!("Failed to import {}", path.display()))?;
    }
    crate::repositories::stat_cache::clear(&tx)?;
    tx.commit()?;
    Ok(())
}

fn import_names(conn: &Connection, path: &Path, table: &str, columns: &[&str]) -> Result<()> {
    let mut reader = csv::Reader::from_path(path)?;
    anyhow::ensure!(
        reader.headers()?.iter().eq(columns.iter().copied()),
        "Expected CSV headers: {}",
        columns.join(",")
    );
    let placeholders = vec!["?"; columns.len()].join(", ");
    let mut stmt = conn.prepare(&format!(
        "INSERT INTO {table} ({}) VALUES ({placeholders})",
        columns.join(", ")
    ))?;
    let mut record = csv::StringRecord::new();
    let mut count = 0_u64;
    while reader.read_record(&mut record)? {
        stmt.execute(rusqlite::params_from_iter(record.iter()))
            .with_context(|| format!("Failed to insert CSV record {}", count + 1))?;
        count += 1;
    }
    info!(file = %path.display(), rows = count, "Imported name CSV");
    Ok(())
}

#[cfg(test)]
mod setup_tests {
    use super::*;

    fn migrations_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")
    }

    const DIAGNOSTIC_ROWS: &str = "
        INSERT INTO game(id,season,round_seq,seq,planned_date,actual_date,away_team_id,home_team_id,game_type)
            VALUES(1,2026,1,1,'2026-04-01','2026-04-01',1,2,'Regular');
        INSERT INTO player_game_fielding_candidate(game_id,count_seq,fielder_position,fielder_id,
            lane_eligible,distance_eligible,evaluated,required_margin_sec,end_reason,selected)
            VALUES(1,1,'CF',99,0,0,0,0.1,'filtered_lane',0);
        INSERT INTO player_game_contact_state VALUES(1,1,1,'test',NULL,'{}');";

    fn diagnostic_fixture() -> (std::path::PathBuf, Connection) {
        let dir=std::env::temp_dir().join(format!("saberbb-setup-diagnostics-{}-{}",std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        for folder in ["ddl","dml","data"] {fs::create_dir_all(dir.join(folder)).unwrap();}
        for table in ["game","first_names","last_names","player_game_fielding_candidate","player_game_contact_state"] {
            let file=format!("{table}_table.sql");
            fs::copy(migrations_dir().join("ddl").join(&file),dir.join("ddl").join(file)).unwrap();
        }
        fs::write(dir.join("data/first_names.csv"),"id,name,reading,gender,country\n1,一郎,,M,jp\n").unwrap();
        fs::write(dir.join("data/last_names.csv"),"id,name,reading,country\n1,田中,,jp\n").unwrap();
        let mut conn=Connection::open_in_memory().unwrap();
        setup_database(&mut conn,&dir).unwrap();
        conn.execute_batch(DIAGNOSTIC_ROWS).unwrap();
        (dir,conn)
    }

    #[test]
    fn repeated_setup_clears_additive_diagnostics_before_reusing_game_ids() {
        let (dir,mut conn)=diagnostic_fixture();
        setup_database(&mut conn,&dir).unwrap();
        for table in ["game","player_game_fielding_candidate","player_game_contact_state"] {
            assert_eq!(conn.query_row(&format!("SELECT COUNT(*) FROM {table}"),[],|r|r.get::<_,i64>(0)).unwrap(),0,"stale {table} rows survived setup");
        }
        // The new game's ID/count/position can be reused without suppressing or
        // overwriting any diagnostic row at save time.
        conn.execute_batch(DIAGNOSTIC_ROWS).unwrap();
        for table in ["player_game_fielding_candidate","player_game_contact_state"] {
            assert_eq!(conn.query_row(&format!("SELECT COUNT(*) FROM {table}"),[],|r|r.get::<_,i64>(0)).unwrap(),1);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_setup_restores_diagnostics_and_original_games() {
        let (dir,mut conn)=diagnostic_fixture();
        fs::write(dir.join("data/last_names.csv"),"wrong,headers\n").unwrap();
        assert!(setup_database(&mut conn,&dir).is_err());
        for table in ["game","player_game_fielding_candidate","player_game_contact_state"] {
            assert_eq!(conn.query_row(&format!("SELECT COUNT(*) FROM {table}"),[],|r|r.get::<_,i64>(0)).unwrap(),1,"{table} was not rolled back");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn orphan_repair_preserves_completed_game_diagnostics_and_game_data() {
        let (dir,conn)=diagnostic_fixture();
        conn.execute_batch("INSERT INTO game(id,planned_date,away_team_id,home_team_id,game_type) VALUES(2,'2026-04-02',1,2,'Regular');
            INSERT INTO player_game_fielding_candidate(game_id,count_seq,fielder_position,fielder_id,lane_eligible,distance_eligible,evaluated,required_margin_sec,end_reason,selected)
            SELECT 2,count_seq,fielder_position,fielder_id,lane_eligible,distance_eligible,evaluated,required_margin_sec,end_reason,selected FROM player_game_fielding_candidate WHERE game_id=1;
            INSERT INTO player_game_fielding_candidate(game_id,count_seq,fielder_position,fielder_id,lane_eligible,distance_eligible,evaluated,required_margin_sec,end_reason,selected)
            SELECT 3,count_seq,fielder_position,fielder_id,lane_eligible,distance_eligible,evaluated,required_margin_sec,end_reason,selected FROM player_game_fielding_candidate WHERE game_id=1;
            INSERT INTO player_game_contact_state SELECT 2,count_seq,state_version,state_source,invalid_reason,state_json FROM player_game_contact_state WHERE game_id=1;
            INSERT INTO player_game_contact_state SELECT 3,count_seq,state_version,state_source,invalid_reason,state_json FROM player_game_contact_state WHERE game_id=1;").unwrap();
        for _ in 0..2 {
            conn.execute_batch(include_str!("../../migrations/upgrades/remove_orphaned_game_diagnostics.sql")).unwrap();
            for table in ["player_game_fielding_candidate","player_game_contact_state"] {
                let ids:Vec<i64>=conn.prepare(&format!("SELECT game_id FROM {table}")).unwrap().query_map([],|r|r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                assert_eq!(ids,vec![1]);
            }
            assert_eq!(conn.query_row("SELECT COUNT(*) FROM game",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn setup_creates_schema_and_loads_all_seed_data() {
        let mut conn = Connection::open_in_memory().unwrap();
        setup_database(&mut conn, &migrations_dir()).unwrap();

        for (table, expected) in [
            ("league", 1),
            ("team", 6),
            ("stadium", 1),
            ("game_season", 1),
            ("first_names", 982864),
            ("last_names", 1568170),
        ] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, expected, "Unexpected seed count for {table}");
        }
        for table in ["normal_param", "gamma_param", "item_weighted"] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert!(count > 0, "Missing seed data for {table}");
        }
        conn.prepare("SELECT * FROM player_info, player_game_batting, player_game_pitching")
            .unwrap();
        let first: (String, String, String, String) = conn
            .query_row(
                "SELECT name, reading, gender, country FROM first_names WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            first,
            ("亜々久".into(), "ああく".into(), "M".into(), "jp".into())
        );
        let last: (String, String, String) = conn
            .query_row(
                "SELECT name, reading, country FROM last_names WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(last, ("亜雁".into(), "あかり".into(), "jp".into()));
    }

    #[test]
    fn setup_imports_quoted_csv_and_rolls_back_on_import_failure() {
        let dir = std::env::temp_dir().join(format!(
            "saberbb-name-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for subdir in ["ddl", "dml", "data"] {
            fs::create_dir_all(dir.join(subdir)).unwrap();
        }
        for table in ["first_names", "last_names"] {
            let file = format!("{table}_table.sql");
            fs::copy(
                migrations_dir().join("ddl").join(&file),
                dir.join("ddl").join(file),
            )
            .unwrap();
        }
        fs::write(
            dir.join("data/first_names.csv"),
            "id,name,reading,gender,country\r\n7,\"Name, With \"\"Quotes\"\"\",,M,us\r\n",
        )
        .unwrap();
        let last_path = dir.join("data/last_names.csv");
        let valid_last = "id,name,reading,country\n9,亜雁,あかり,jp\n";
        fs::write(&last_path, valid_last).unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        // Repeated setup resets the tables without duplicating names.
        for _ in 0..2 {
            setup_database(&mut conn, &dir).unwrap();
            let name: (i64, String, String) = conn
                .query_row("SELECT id, name, reading FROM first_names", [], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .unwrap();
            assert_eq!(name, (7, "Name, With \"Quotes\"".into(), "".into()));
        }
        conn.execute("UPDATE first_names SET name = 'Original'", [])
            .unwrap();
        for invalid in [
            Some("wrong,headers\n1,Name\n"),
            Some("id,name,reading,country\n9,Name,,us\n10,Missing\n"),
            Some("id,name,reading,country\n9,Name,,us\n9,Duplicate,,us\n"),
            None,
        ] {
            match invalid {
                Some(csv) => fs::write(&last_path, csv).unwrap(),
                None => fs::remove_file(&last_path).unwrap(),
            }
            let err = setup_database(&mut conn, &dir).unwrap_err();
            assert!(err.to_string().contains("last_names.csv"));
            let name: String = conn
                .query_row("SELECT name FROM first_names WHERE id = 7", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(name, "Original");
            let name: String = conn
                .query_row("SELECT name FROM last_names WHERE id = 9", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(name, "亜雁");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn setup_rolls_back_schema_and_data_on_sql_failure() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE batter_info (id INTEGER);
             INSERT INTO batter_info VALUES (42);
             CREATE VIEW league AS SELECT 1 AS id;",
        )
        .unwrap();

        let err = setup_database(&mut conn, &migrations_dir()).unwrap_err();
        assert!(err.to_string().contains("league_table.sql"));
        let id: i64 = conn
            .query_row("SELECT id FROM batter_info", [], |row| row.get(0))
            .unwrap();
        assert_eq!(id, 42);
        assert!(conn.prepare("SELECT * FROM count").is_err());
    }
}

#[cfg(test)]
mod read_only_tests {
    use super::*;
    #[test]
    fn diagnostic_pool_rejects_writes_and_never_creates_a_missing_database() {
        let dir=std::env::temp_dir().join(format!("saberbb-read-only-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&dir).unwrap();let path=dir.join("source.db");
        let conn=Connection::open(&path).unwrap();conn.execute_batch("CREATE TABLE t(value);INSERT INTO t VALUES(1);").unwrap();drop(conn);
        let db=SqlDb::read_only(&path).unwrap();let conn=db.get_conn().unwrap();
        assert_eq!(conn.query_row("SELECT value FROM t",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        assert!(conn.execute("UPDATE t SET value=2",[]).is_err());
        drop(conn);drop(db);
        let missing=dir.join("missing.db");let db=SqlDb::read_only(&missing).unwrap();
        assert!(db.get_conn().is_err());assert!(!missing.exists());drop(db);
        fs::remove_dir_all(dir).unwrap();
    }
}

pub fn maintenance() -> Result<()> {
    let conn = SqlDb::new()?.get_conn()?;

    // Vacuum in case there are many blank pages
    let freelist: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;

    if freelist as f64 / page_count as f64 > 0.2 {
        info!("run a vacuum process to fix database bloat.");
        conn.execute("VACUUM;", [])?;
    }

    conn.execute("PRAGMA optimize;", [])?;
    Ok(())
}
