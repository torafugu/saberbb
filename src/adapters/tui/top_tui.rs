use super::app::App;
use crate::config::AppConfig;
use color_eyre::Result;

const TICK_RATE: f64 = 1.0;
const FRAME_RATE: f64 = 1.0;

#[tokio::main]
pub async fn menu(config: AppConfig) -> Result<()> {
    super::errors::init()?;
    let mut app = App::new(TICK_RATE, FRAME_RATE, config)?;
    app.run().await?;
    Ok(())
}
