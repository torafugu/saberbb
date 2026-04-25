use anyhow::Result;
use directories::ProjectDirs;
use rusqlite::Connection;
use std::fs;

const ERROR_NO_DATA_DIR: &str = "The data directory is not found.";

pub fn get_db_conn() -> Result<Connection> {
    let proj_dirs = ProjectDirs::from("jp", "cosmi", "statbb").expect(ERROR_NO_DATA_DIR);

    let data_dir = proj_dirs.data_dir();
    fs::create_dir_all(data_dir)?;

    let db_path = data_dir.join("statbb.db");

    Ok(Connection::open(&db_path)?)
}
