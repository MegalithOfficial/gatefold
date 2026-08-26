use anyhow::Result;
use librespot::{
    core::SessionConfig,
    oauth::{OAuthClientBuilder, OAuthToken},
};

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

pub fn web_client_id() -> Option<String> {
    std::env::var("GATEFOLD_CLIENT_ID")
        .ok()
        .or_else(|| {
            let path = dirs::config_dir()?.join("gatefold/client_id");
            std::fs::read_to_string(path).ok()
        })
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
}
