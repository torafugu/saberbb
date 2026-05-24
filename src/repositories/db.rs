use anyhow::Result;
use deadpool::managed::{Manager, Metrics, Object, Pool, RecycleResult};
use directories::ProjectDirs;
use rusqlite::Connection;
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
