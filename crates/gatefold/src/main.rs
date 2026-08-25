mod ui;

use anyhow::{Context, Result};
use relm4::RelmApp;
use tracing_subscriber::EnvFilter;

const APP_ID: &str = "io.github.megalithofficial.gatefold";

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let uri = std::env::args()
        .nth(1)
        .context("usage: gatefold <spotify track uri>")?;

    RelmApp::new(APP_ID)
        .with_args(Vec::new())
        .run::<ui::Gatefold>(uri);

    Ok(())
}
