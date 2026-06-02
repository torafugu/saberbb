use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug)]
pub struct AppConfig {
    pub version: u8,
    pub language: String,
    pub database_path: PathBuf,
}
impl Default for AppConfig {
    fn default() -> Self {
        let proj_dirs =
            ProjectDirs::from("jp", "cosmi", "saberbb").expect("Data directory is not found");
        Self {
            version: 1,
            language: String::from("en-US"),
            database_path: proj_dirs.config_dir().join("saberbb.db"),
        }
    }
}

pub fn load_app_config() -> anyhow::Result<AppConfig> {
    let app_config = confy::load::<AppConfig>("saberbb", None).unwrap_or_default();
    Ok(app_config)
}
