use anyhow::{Context, Result};
use gatefold_core::{cache_dir, metadata, player, session};
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

    let track = metadata::track(&session, &uri).await?;
    let artists: Vec<&str> = track.artists.iter().map(|a| a.name.as_str()).collect();
    tracing::info!("{} by {} ({})", track.name, artists.join(", "), track.album.name);

    let cover = metadata::cover(&session, &track).await?;
    let path = cache_dir()?.join("cover.jpg");
    std::fs::write(&path, &cover)?;
    tracing::info!("cover: {} bytes to {}", cover.len(), path.display());

    player::play(session, &uri).await
}
