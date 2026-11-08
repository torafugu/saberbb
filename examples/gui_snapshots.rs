//! Render the real Dioxus view from the configured database for headless visual QA.
use saberbb::{AppContext,init_app_context,init_dirs,I18nManager};
use saberbb::repositories::{game_repository::SqlGameRepository,player_repository::SqlPlayerRepository,
    schedule_repository::SqlScheduleRepository,statistics_repository::SqlStatRepository};
#[tokio::main]
async fn main()->anyhow::Result<()> {
    init_dirs()?;let config=saberbb::config::load_app_config()?;let _=I18nManager::global().lang_db(); I18nManager::init(&config.language);
    init_app_context(AppContext {game_repository:SqlGameRepository::new()?,player_repository:SqlPlayerRepository::new()?,schedule_repository:SqlScheduleRepository::new()?,statistics_repository:SqlStatRepository::new()?});
    let output=std::env::args().nth(1).unwrap_or_else(||"/tmp/saberbb-gui-snapshots".into());
    if std::env::args().nth(2).as_deref()==Some("--management") {
        saberbb::adapters::gui::snapshot_management(std::path::Path::new(&output)).await?;
    } else {
        saberbb::adapters::gui::snapshot_database(std::path::Path::new(&output)).await?;
    }
    println!("GUI snapshots: {output}");Ok(())
}
