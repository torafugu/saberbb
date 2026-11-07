//! Local-only browser harness for exercising the exact desktop GUI components.
//! GUI widgets and database queries are shared; this is not a second UI.
use axum::{extract::ws::WebSocketUpgrade, response::Html, routing::get, Router};
use saberbb::{AppContext, init_app_context, init_dirs, I18nManager};
use saberbb::repositories::{game_repository::SqlGameRepository, player_repository::SqlPlayerRepository,
    schedule_repository::SqlScheduleRepository, statistics_repository::SqlStatRepository};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_dirs()?;
    let config = saberbb::config::load_app_config()?;
    I18nManager::init(&config.language);
    init_app_context(AppContext { game_repository:SqlGameRepository::new()?,player_repository:SqlPlayerRepository::new()?,
        schedule_repository:SqlScheduleRepository::new()?,statistics_repository:SqlStatRepository::new()? });
    let pool = dioxus_liveview::LiveViewPool::new();
    let router = Router::new()
        .route("/", get(|| async { Html(format!("<!doctype html><html lang=\"ja\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>SABERBB GUI Test</title></head><body><div id=\"main\"></div>{}</body></html>",dioxus_liveview::interpreter_glue("ws://127.0.0.1:8787/ws"))) }))
        .route("/ws", get(move |ws: WebSocketUpgrade| async move {
            ws.on_upgrade(move |socket| async move {
                let _ = pool.launch(dioxus_liveview::axum_socket(socket), saberbb::adapters::gui::App).await;
            })
        }));
    let listener=tokio::net::TcpListener::bind("127.0.0.1:8787").await?;
    println!("GUI test harness: http://127.0.0.1:8787 (local machine only)");
    axum::serve(listener,router).await?;
    Ok(())
}
