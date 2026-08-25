use anyhow::Result;
use gatefold_core::auth;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let token = auth::login().await?;
    tracing::info!("logged in, scopes: {}", token.scopes.join(" "));

    Ok(())
}
