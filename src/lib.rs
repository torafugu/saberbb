pub mod adapters;
pub mod config;
pub mod domain;
pub mod error;
pub mod i18n;
pub mod repositories;

use anyhow::Result;
use directories::ProjectDirs;
use repositories::game_repository::SqlGameRepository;
use repositories::player_repository::SqlPlayerRepository;
use repositories::schedule_repository::SqlScheduleRepository;
use repositories::statistics_repository::SqlStatRepository;
use std::fs;
use std::sync::OnceLock;

pub use i18n::I18nManager;

static APP_CONTEXT: OnceLock<AppContext> = OnceLock::new();
static PROJ_DIRS: OnceLock<ProjectDirs> = OnceLock::new();

pub fn proj_dirs() -> &'static ProjectDirs {
    PROJ_DIRS.get_or_init(|| {
        ProjectDirs::from("jp", "cosmi", "saberbb").expect("Project directory is not found")
    })
}

pub fn init_dirs() -> Result<()> {
    fs::create_dir_all(proj_dirs().data_dir())?;
    fs::create_dir_all(proj_dirs().config_dir())?;
    fs::create_dir_all(proj_dirs().cache_dir())?;
    fs::create_dir_all(proj_dirs().data_local_dir())?;
    Ok(())
}

pub fn app_context() -> &'static AppContext {
    APP_CONTEXT.get().expect("APP_CONTEXT is not initialized")
}

// 初期化用（main.rsから呼ぶ）
pub fn init_app_context(ctx: AppContext) {
    let _ = APP_CONTEXT.set(ctx);
}

pub struct AppContext {
    pub game_repository: SqlGameRepository,
    pub player_repository: SqlPlayerRepository,
    pub schedule_repository: SqlScheduleRepository,
    pub statistics_repository: SqlStatRepository,
}
