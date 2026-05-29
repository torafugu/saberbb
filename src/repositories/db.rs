use crate::error::AppError;
use anyhow::Result;
use deadpool::managed::{Manager, Metrics, Object, Pool, RecycleResult};
use directories::ProjectDirs;
use rusqlite::Connection;
use rusqlite::types::FromSql;
use std::fs;

#[derive(Clone)]
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

#[derive(Clone)]
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
        futures::executor::block_on(self.pool.get())
            .map_err(|e| Box::new(e) as _)
            .map_err(DbError::Pool)
    }
}

pub fn create_sqlite_pool() -> Result<Pool<SqliteManager>> {
    let proj_dirs =
        ProjectDirs::from("jp", "cosmi", "saberbb").expect("Data directory is not found");

    let data_dir = proj_dirs.data_dir();
    fs::create_dir_all(data_dir)?;

    let db_path = data_dir.join("saberbb.db");

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

#[derive(Clone)]
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

        let mut mapped_rows = stmt
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
