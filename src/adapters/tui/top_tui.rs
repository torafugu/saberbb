use super::app::App;
use color_eyre::Result;

const TICK_RATE: f64 = 1.0;
const FRAME_RATE: f64 = 1.0;

#[tokio::main]
pub async fn menu() -> Result<()> {
    // crate::errors::init()?;
    // crate::logging::init()?;

    // let args = Cli::parse();
    let mut app = App::new(TICK_RATE, FRAME_RATE)?;
    app.run().await?;
    Ok(())
}
