use super::app::App;
use crate::config::AppConfig;
use color_eyre::Result;

#[tokio::main]
pub async fn menu(config: AppConfig) -> Result<()> {
    super::errors::init()?;
    let mut app = App::new(config)?;
    app.run().await?;
    Ok(())
}
