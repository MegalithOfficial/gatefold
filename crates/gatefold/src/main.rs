use anyhow::Result;
use gatefold_core::session;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let session = session::connect().await?;
    tracing::info!("connected as {}", session.username());

    Ok(())
}
