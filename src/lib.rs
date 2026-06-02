pub mod adapters;
pub mod config;
pub mod domain;
pub mod error;
pub mod i18n;
pub mod repositories;

use repositories::game_repository::SqlGameRepository;
use repositories::player_repository::SqlPlayerRepository;
use repositories::schedule_repository::SqlScheduleRepository;
use repositories::statistics_repository::SqlStatRepository;
use std::sync::OnceLock;

pub use i18n::I18nManager;

static APP_CONTEXT: OnceLock<AppContext> = OnceLock::new();

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
