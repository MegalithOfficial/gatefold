use anyhow::{Context, Result};
use gatefold_core::{player, session};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let uri = std::env::args()
        .nth(1)
        .context("usage: gatefold <spotify track uri>")?;

    let session = session::connect().await?;
    tracing::info!("connected as {}", session.username());

    player::play(session, &uri).await
}
