mod app;
mod components;
mod css;
mod pages;
mod palette;
mod shortcuts;
mod square;

use anyhow::Result;
use relm4::RelmApp;
use tracing_subscriber::EnvFilter;

const APP_ID: &str = "io.github.megalithofficial.gatefold";

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let app = RelmApp::new(APP_ID).with_args(Vec::new());

    app.run::<app::App>(());

    Ok(())
}
