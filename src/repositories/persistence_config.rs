use crate::t;
use anyhow::Result;
use deadpool::managed::{Manager, Metrics, RecycleResult};
use directories::ProjectDirs;
use rusqlite::Connection;
use std::fs;

#[derive(Clone)]
pub struct SqliteManager {
    pub path: std::path::PathBuf,
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
        Ok(())
    }
}

pub fn get_sqlite_manager() -> Result<SqliteManager> {
    let proj_dirs = ProjectDirs::from("jp", "cosmi", "statbb").expect(&t!("error_no_data_dir"));

    let data_dir = proj_dirs.data_dir();
    fs::create_dir_all(data_dir)?;

    let db_path = data_dir.join("statbb.db");
    let manager = SqliteManager { path: db_path };

    Ok(manager)
}
