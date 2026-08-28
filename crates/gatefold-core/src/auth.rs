use std::{
    fs::OpenOptions,
    net::SocketAddr,
    sync::LazyLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use librespot::{
    core::SessionConfig,
    oauth::{OAuthClientBuilder, OAuthToken},
};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, RedirectUrl, Scope,
    TokenResponse, TokenUrl, basic::BasicClient,
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::Mutex,
};
use url::Url;

pub const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";

pub const SCOPES: &[&str] = &["streaming", "user-read-email", "user-read-private"];

static WEB_AUTH: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

pub fn client_id() -> String {
    SessionConfig::default().client_id
}

pub async fn login() -> Result<OAuthToken> {
    let token = authorize(&client_id(), SCOPES).await?;
    if let Err(error) =
        CachedToken::from_oauth(token.clone(), None, &client_id()).store("session-auth")
    {
        tracing::warn!("could not cache Spotify OAuth token: {error}");
    }

    Ok(token)
}

async fn authorize(client_id: &str, scopes: &[&str]) -> Result<OAuthToken> {
    let client = BasicClient::new(ClientId::new(client_id.to_owned()))
        .set_auth_uri(AuthUrl::new(
            "https://accounts.spotify.com/authorize".to_owned(),
        )?)
        .set_token_uri(TokenUrl::new(
            "https://accounts.spotify.com/api/token".to_owned(),
        )?)
        .set_redirect_uri(RedirectUrl::new(REDIRECT_URI.to_owned())?);
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, state) = client
        .authorize_url(CsrfToken::new_random)
        .add_scopes(scopes.iter().map(|scope| Scope::new((*scope).to_owned())))
        .set_pkce_challenge(challenge)
        .url();

    let redirect = Url::parse(REDIRECT_URI)?;
    let address = SocketAddr::new(
        redirect.host_str().context("redirect host")?.parse()?,
        redirect.port().context("redirect port")?,
    );
    let listener = TcpListener::bind(address).await?;
    tracing::info!("waiting for Spotify at {address}");
    open::that_in_background(url.as_str());

    let code = loop {
        let (stream, _) = listener.accept().await?;
        let mut stream = BufReader::new(stream);
        let mut request = String::new();
        stream.read_line(&mut request).await?;
        let Some(path) = request.split_whitespace().nth(1) else {
            continue;
        };
        let callback = Url::parse(&format!("http://localhost{path}"))?;
        let query = |key: &str| {
            callback
                .query_pairs()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.into_owned())
        };
        let (Some(code), Some(returned)) = (query("code"), query("state")) else {
            continue;
        };
        let body = "You can go back to Gatefold.";
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.get_mut().write_all(response.as_bytes()).await?;
        if returned != *state.secret() {
            bail!("Spotify returned a different state");
        }
        break code;
    };

    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(verifier)
        .request_async(&http)
        .await?;

    Ok(OAuthToken {
        access_token: response.access_token().secret().clone(),
        refresh_token: response
            .refresh_token()
            .map(|token| token.secret().clone())
            .unwrap_or_default(),
        expires_at: Instant::now()
            + response
                .expires_in()
                .unwrap_or_else(|| Duration::from_secs(3600)),
        token_type: format!("{:?}", response.token_type()),
        scopes: response
            .scopes()
            .map(|granted| granted.iter().map(|scope| scope.to_string()).collect())
            .unwrap_or_else(|| scopes.iter().map(|scope| (*scope).to_owned()).collect()),
    })
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
        authorize(&client_id, &scopes).await?
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
