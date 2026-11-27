use crate::proj_dirs;
use anyhow::{Context, ensure};
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
    #[serde(default)]
    pub simulation: SimulationConfig,
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
            simulation: SimulationConfig::default(),
        }
    }
}

pub fn load_app_config() -> anyhow::Result<AppConfig> {
    let mut app_config = load_saved_app_config()?;
    if let Some(path) = std::env::var_os("SABERBB_DB") {
        anyhow::ensure!(
            !path.to_string_lossy().trim().is_empty(),
            "SABERBB_DB is blank"
        );
        app_config.database_path = PathBuf::from(path);
    }
    Ok(app_config)
}

fn load_saved_app_config() -> anyhow::Result<AppConfig> {
    load_config_path(&proj_dirs().config_dir().join("default-config.toml"))
}

fn load_config_path(path: &std::path::Path) -> anyhow::Result<AppConfig> {
    let mut config = confy::load_path::<AppConfig>(path)
        .with_context(|| format!("Loading {}", path.display()))?;
    config.simulation.validate()?;
    merge_default_keybindings(&mut config.keybindings);
    Ok(config)
}

fn merge_default_keybindings(keybindings: &mut KeybindingConfig) {
    for (mode, defaults) in default_keybindings() {
        let mode_keybindings = keybindings.entry(mode).or_default();
        for (key, action) in defaults {
            mode_keybindings.entry(key).or_insert(action);
        }
    }
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
            ("<right>".to_string(), "NextCount".to_string()),
            ("<left>".to_string(), "PreviousCount".to_string()),
            ("<esc>".to_string(), "Back".to_string()),
            ("<backspace>".to_string(), "Back".to_string()),
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
    proj_dirs().config_dir().join("saberbb.db")
}

/// Update only the language preference, retaining database and keybinding settings.
pub fn save_language(language: &str) -> anyhow::Result<()> {
    let mut config = load_saved_app_config()?;
    config.language = language.into();
    confy::store_path(
        proj_dirs().config_dir().join("default-config.toml"),
        &config,
    )?;
    Ok(())
}

/// Settings for scheduled game processing and analysis.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct SimulationConfig {
    pub swing_speed_bonus_ms: f64,
    pub swing_speed_cap_ms: f64,
    pub swing_execution: String,
    pub catcher_blocking: bool,
    pub pitch_border_1_2: bool,
    pub pitch_third_base_risk_weight: f64,
    pub pitch_location_error_floor: f64,
    pub pitch_location_irreducible_error: f64,
    pub pitch_control_cap: Option<f64>,
    pub pitch_control_above_cap_scale: f64,
    pub pitch_around_0_2_edge_scale: f64,
    pub pitch_execution_fatigue: bool,
    pub scheduled_game_seed: Option<u64>,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            swing_speed_bonus_ms: 0.0,
            swing_speed_cap_ms: 34.0,
            swing_execution: "mixture".into(),
            catcher_blocking: true,
            pitch_border_1_2: false,
            pitch_third_base_risk_weight: 0.0,
            pitch_location_error_floor: 0.0,
            pitch_location_irreducible_error: 0.20,
            pitch_control_cap: None,
            pitch_control_above_cap_scale: 0.0,
            pitch_around_0_2_edge_scale: 1.0,
            pitch_execution_fatigue: false,
            scheduled_game_seed: None,
        }
    }
}

impl SimulationConfig {
    pub fn swing_execution_model(
        &self,
    ) -> anyhow::Result<crate::domain::resolver::batting_resolver::SwingExecutionModel> {
        use crate::domain::resolver::batting_resolver::SwingExecutionModel;
        match self.swing_execution.as_str() {
            "legacy" => Ok(SwingExecutionModel::Legacy),
            "gaussian" => Ok(SwingExecutionModel::Gaussian),
            "mixture" => Ok(SwingExecutionModel::Mixture),
            _ => anyhow::bail!("simulation.swing_execution must be legacy, gaussian, or mixture"),
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        self.swing_execution_model()?;
        ensure!(
            self.swing_speed_bonus_ms.is_finite() && self.swing_speed_bonus_ms >= 0.0,
            "simulation.swing_speed_bonus_ms must be finite and nonnegative"
        );
        ensure!(
            self.swing_speed_cap_ms.is_finite() && self.swing_speed_cap_ms > 0.0,
            "simulation.swing_speed_cap_ms must be finite and positive"
        );
        for (name, value, max) in [
            (
                "pitch_third_base_risk_weight",
                self.pitch_third_base_risk_weight,
                10.0,
            ),
            (
                "pitch_location_error_floor",
                self.pitch_location_error_floor,
                0.5,
            ),
            (
                "pitch_location_irreducible_error",
                self.pitch_location_irreducible_error,
                0.5,
            ),
            (
                "pitch_control_above_cap_scale",
                self.pitch_control_above_cap_scale,
                1.0,
            ),
            (
                "pitch_around_0_2_edge_scale",
                self.pitch_around_0_2_edge_scale,
                1.0,
            ),
        ] {
            ensure!(
                (0.0..=max).contains(&value),
                "simulation.{name} must be between 0 and {max}"
            );
        }
        ensure!(
            self.pitch_control_cap.is_none_or(f64::is_finite),
            "simulation.pitch_control_cap must be finite"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn read(text: &str) -> anyhow::Result<AppConfig> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "saberbb-config-{}-{}.toml",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, text)?;
        let result = load_config_path(&path);
        std::fs::remove_file(path)?;
        result
    }

    #[test]
    fn old_config_and_template_preserve_defaults() {
        for text in ["language = 'ja-JP'", include_str!("../default-config.toml")] {
            let config = read(text).unwrap();
            assert_eq!(
                serde_json::to_value(config.simulation).unwrap(),
                serde_json::to_value(SimulationConfig::default()).unwrap()
            );
            assert!(config.keybindings["Home"].contains_key("<q>"));
        }
    }

    #[test]
    fn simulation_overrides_survive_serialization() {
        let config = read("language = 'ja-JP'\n[simulation]\ncatcher_blocking = false\nswing_execution = 'gaussian'\npitch_control_cap = 1.5\nscheduled_game_seed = 42").unwrap();
        assert!(!config.simulation.catcher_blocking);
        assert_eq!(config.simulation.scheduled_game_seed, Some(42));
        assert_eq!(config.simulation.pitch_control_cap, Some(1.5));
        assert_eq!(
            config.simulation.swing_execution_model().unwrap(),
            crate::domain::resolver::batting_resolver::SwingExecutionModel::Gaussian
        );
        let path = std::env::temp_dir().join(format!(
            "saberbb-config-roundtrip-{}.toml",
            std::process::id()
        ));
        confy::store_path(&path, &config).unwrap();
        let saved = load_config_path(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            serde_json::to_value(config).unwrap(),
            serde_json::to_value(saved).unwrap()
        );
    }

    #[test]
    fn invalid_settings_are_not_silently_defaulted() {
        for setting in [
            "swing_execution = 'invalid'",
            "catcher_blocking = 1",
            "swing_speed_bonus_ms = -1.0",
            "swing_speed_bonus_ms = inf",
            "swing_speed_cap_ms = 0.0",
            "swing_speed_cap_ms = nan",
            "pitch_third_base_risk_weight = 11.0",
            "pitch_location_error_floor = -0.1",
            "pitch_location_irreducible_error = 0.6",
            "pitch_control_cap = inf",
            "pitch_control_above_cap_scale = 1.1",
            "pitch_around_0_2_edge_scale = -0.1",
            "scheduled_game_seed = -1",
            "catcher_bloking = true",
        ] {
            assert!(
                read(&format!("[simulation]\n{setting}")).is_err(),
                "{setting}"
            );
        }
        assert!(read("invalid toml [").is_err());
    }
}
