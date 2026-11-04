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
}

impl SqliteManager {
    #[cfg(test)]
    pub(crate) fn from_path(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl Manager for SqliteManager {
    type Type = Connection;
    type Error = rusqlite::Error;

    async fn create(&self) -> Result<Connection, rusqlite::Error> {
        Connection::open(&self.path)
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

    let manager = SqliteManager { path: db_path };

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

/// Run the schema and seed scripts against the configured database.
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
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod setup_tests {
    use super::*;

    fn migrations_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")
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
