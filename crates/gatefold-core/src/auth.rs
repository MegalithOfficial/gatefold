use std::{
    fs::OpenOptions,
    sync::LazyLock,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use librespot::{
    core::SessionConfig,
    oauth::{OAuthClientBuilder, OAuthToken},
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";

pub const SCOPES: &[&str] = &["streaming", "user-read-email", "user-read-private"];

static WEB_AUTH: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

pub fn client_id() -> String {
    SessionConfig::default().client_id
}

pub async fn login() -> Result<OAuthToken> {
    let client = OAuthClientBuilder::new(&client_id(), REDIRECT_URI, SCOPES.to_vec())
        .open_in_browser()
        .build()?;
    let token = client.get_access_token_async().await?;
    if let Err(error) =
        CachedToken::from_oauth(token.clone(), None, &client_id()).store("session-auth")
    {
        tracing::warn!("could not cache Spotify OAuth token: {error}");
    }

    Ok(token)
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

pub async fn web_access_token() -> Result<String> {
    let _guard = WEB_AUTH.lock().await;
    let custom_client_id = web_client_id();
    let client_id = custom_client_id.clone().unwrap_or_else(client_id);
    let cache_name = if custom_client_id.is_some() {
        "web-auth"
    } else {
        "session-auth"
    };
    let cached = CachedToken::load(cache_name).filter(|token| token.client_id == client_id);
    if let Some(token) = cached.as_ref().filter(|token| !token.expires_soon()) {
        return Ok(token.access_token.clone());
    }

    let scopes = if custom_client_id.is_none() {
        SCOPES.to_vec()
    } else {
        Vec::new()
    };
    let builder = || OAuthClientBuilder::new(&client_id, REDIRECT_URI, scopes.clone());

    let token = if let Some(refresh_token) = cached
        .as_ref()
        .map(|token| token.refresh_token.as_str())
        .filter(|token| !token.is_empty())
    {
        builder()
            .build()?
            .refresh_token_async(refresh_token)
            .await?
    } else {
        builder()
            .open_in_browser()
            .build()?
            .get_access_token_async()
            .await?
    };
    let token = CachedToken::from_oauth(token, cached.as_ref(), &client_id);
    token.store(cache_name)?;

    Ok(token.access_token)
}

#[derive(Serialize, Deserialize)]
struct CachedToken {
    #[serde(default)]
    client_id: String,
    access_token: String,
    refresh_token: String,
    expires_at: u64,
}

impl CachedToken {
    fn from_oauth(token: OAuthToken, previous: Option<&Self>, client_id: &str) -> Self {
        let refresh_token = if token.refresh_token.is_empty() {
            previous
                .map(|token| token.refresh_token.clone())
                .unwrap_or_default()
        } else {
            token.refresh_token
        };
        let expires_in = token.expires_at.saturating_duration_since(Instant::now());
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .saturating_add(expires_in)
            .as_secs();

        Self {
            client_id: client_id.to_owned(),
            access_token: token.access_token,
            refresh_token,
            expires_at,
        }
    }

    fn expires_soon(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.expires_at <= now.saturating_add(60)
    }

    fn load(name: &str) -> Option<Self> {
        let path = crate::cache_dir().ok()?.join(format!("{name}.json"));
        let json = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&json).ok()
    }

    fn store(&self, name: &str) -> Result<()> {
        let dir = crate::cache_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{name}.json"));
        let staging = path.with_extension("part");
        let json = serde_json::to_vec(self)?;

        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        std::io::Write::write_all(&mut options.open(&staging)?, &json)?;
        std::fs::rename(staging, path)?;
        Ok(())
    }
}
