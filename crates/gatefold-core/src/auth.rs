use anyhow::Result;
use librespot::core::SessionConfig;
use librespot::oauth::{OAuthClientBuilder, OAuthToken};

pub const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";

pub const SCOPES: &[&str] = &["streaming", "user-read-email", "user-read-private"];

pub fn client_id() -> String {
    SessionConfig::default().client_id
}

pub async fn login() -> Result<OAuthToken> {
    let client = OAuthClientBuilder::new(&client_id(), REDIRECT_URI, SCOPES.to_vec())
        .open_in_browser()
        .build()?;

    Ok(client.get_access_token_async().await?)
}
