use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub type KeybindingConfig = HashMap<String, HashMap<String, String>>;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AppConfig {
    #[serde(default = "default_version")]
    pub version: u8,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_tick_rate")]
    pub tick_rate: f64,
    #[serde(default = "default_frame_rate")]
    pub frame_rate: f64,
    #[serde(default = "default_database_path")]
    pub database_path: PathBuf,
    #[serde(default = "default_keybindings")]
    pub keybindings: KeybindingConfig,
}
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            language: default_language(),
            tick_rate: default_tick_rate(),
            frame_rate: default_frame_rate(),
            database_path: default_database_path(),
            keybindings: default_keybindings(),
        }
    }
}

pub fn load_app_config() -> anyhow::Result<AppConfig> {
    let app_config = confy::load::<AppConfig>("saberbb", None).unwrap_or_default();
    Ok(app_config)
}

fn default_keybindings() -> KeybindingConfig {
    HashMap::from([(
        "Home".to_string(),
        HashMap::from([
            ("<q>".to_string(), "Quit".to_string()),
            ("<Ctrl-d>".to_string(), "Quit".to_string()),
            ("<Ctrl-c>".to_string(), "Quit".to_string()),
            ("<Ctrl-z>".to_string(), "Suspend".to_string()),
            ("<down>".to_string(), "SelectNext".to_string()),
            ("<up>".to_string(), "SelectPrevious".to_string()),
            ("<enter>".to_string(), "ConfirmSelection".to_string()),
        ]),
    )])
}

fn default_version() -> u8 {
    1
}

fn default_language() -> String {
    String::from("en-US")
}

fn default_tick_rate() -> f64 {
    1.0
}

fn default_frame_rate() -> f64 {
    1.0
}

fn default_database_path() -> PathBuf {
    let proj_dirs =
        ProjectDirs::from("jp", "cosmi", "saberbb").expect("Data directory is not found");
    proj_dirs.config_dir().join("saberbb.db")
}
