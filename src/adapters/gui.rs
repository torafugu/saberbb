//! Dioxus is the single results/statistics UI. Desktop loads the same local DB as the CLI.
mod app;
mod locale;
pub use app::App;
mod tables;
mod game;
mod teams;

#[cfg(feature = "desktop")]
pub fn launch() {
    use dioxus::desktop::{Config, WindowBuilder, LogicalSize};
    dioxus::LaunchBuilder::desktop().with_cfg(Config::new().with_window(
        WindowBuilder::new().with_title("SABERBB — Game & Statistics").with_inner_size(LogicalSize::new(1440.0, 960.0)),
    )).launch(app::App);
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(work).await.map_err(|e| e.to_string())?.map_err(|e| format!("{e:#}"))
}

#[cfg(feature = "gui-tests")]
mod harness;
#[cfg(feature = "gui-tests")]
pub use harness::snapshot_database;
