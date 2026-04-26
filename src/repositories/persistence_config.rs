use crate::t;
use anyhow::Result;
use directories::ProjectDirs;
use rusqlite::Connection;
use std::fs;

pub fn get_db_conn() -> Result<Connection> {
    let proj_dirs = ProjectDirs::from("jp", "cosmi", "statbb").expect(&t!("error_no_data_dir"));

    let data_dir = proj_dirs.data_dir();
    fs::create_dir_all(data_dir)?;

    let db_path = data_dir.join("statbb.db");

    Ok(Connection::open(&db_path)?)
}
