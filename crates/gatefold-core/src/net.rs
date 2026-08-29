use std::{fmt::Display, future::Future, sync::LazyLock, time::Duration};

use anyhow::{Context, Result};
use http::{Method, Request, header};
use librespot::core::{Session, error::ErrorKind};
use tokio::sync::Semaphore;
use url::Url;

use crate::auth;

const CONCURRENCY: usize = 32;
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36";
const API_USER_AGENT: &str = concat!(
    "Gatefold/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/MegalithOfficial/gatefold)"
);
const RETRIES: u32 = 3;

static PERMITS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(CONCURRENCY));
static WEB_API: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);
static PUBLIC_API: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(API_USER_AGENT)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("public API client")
});
static WEB_API_GATE: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

pub trait Recoverable {
    fn recoverable(&self) -> bool;
}

impl Recoverable for reqwest::Error {
    fn recoverable(&self) -> bool {
        self.is_timeout() || self.is_connect()
    }
}

impl Recoverable for librespot::core::Error {
    fn recoverable(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Unavailable | ErrorKind::DeadlineExceeded
        )
    }
}

pub async fn fetch<T, E, F, Fut>(op: F) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: Recoverable + Display,
{
    let _permit = PERMITS.acquire().await.expect("fetch semaphore closed");

    let mut attempt = 0;
    loop {
        match op().await {
            Ok(value) => return Ok(value),
            Err(error) if attempt < RETRIES && error.recoverable() => {
                attempt += 1;
                let wait = Duration::from_millis(500 * 4u64.pow(attempt - 1));
                tracing::warn!("request limited ({error}), retrying in {wait:?}");
                tokio::time::sleep(wait).await;
            }
            Err(error) => return Err(error),
        }
    }
}

pub(crate) async fn web_api(_session: &Session, url: &Url) -> Result<Vec<u8>> {
    let access_token = auth::web_access_token().await?;
    let _gate = WEB_API_GATE.lock().await;

    for attempt in 0..=RETRIES {
        let response = fetch(|| WEB_API.get(url.clone()).bearer_auth(&access_token).send())
            .await
            .with_context(|| format!("Spotify Web API request failed: {}", url.path()))?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < RETRIES {
            let wait = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(30))
                .max(Duration::from_secs(1));
            tracing::warn!("Spotify Web API rate limited, retrying in {wait:?}");
            tokio::time::sleep(wait).await;
            continue;
        }

        let response = response
            .error_for_status()
            .with_context(|| format!("Spotify Web API request failed: {}", url.path()))?;
        return Ok(response.bytes().await?.to_vec());
    }

    unreachable!("Web API retry loop always returns on its final attempt")
}

pub(crate) async fn partner_api(session: &Session, url: &Url) -> Result<Vec<u8>> {
    let token = session.login5().auth_token().await?;
    let client_token = session.spclient().client_token().await?;

    get(
        session,
        url,
        &[
            (
                header::AUTHORIZATION.as_str(),
                &format!("Bearer {}", token.access_token),
            ),
            ("client-token", &client_token),
            ("app-platform", "WebPlayer"),
        ],
    )
    .await
    .context("Spotify partner request failed")
}

pub(crate) async fn public_api(url: &Url) -> Result<Option<Vec<u8>>> {
    for attempt in 0..=RETRIES {
        let response = fetch(|| PUBLIC_API.get(url.clone()).send())
            .await
            .with_context(|| format!("public API request failed: {url}"))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < RETRIES {
            let wait = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(2))
                .max(Duration::from_secs(1));
            tracing::warn!("public API rate limited, retrying in {wait:?}");
            tokio::time::sleep(wait).await;
            continue;
        }

        return Ok(Some(response.error_for_status()?.bytes().await?.to_vec()));
    }

    unreachable!("public API retry loop always returns on its final attempt")
}

pub(crate) async fn page(url: &Url) -> Result<String> {
    static BROWSER: LazyLock<reqwest::Client> = LazyLock::new(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("browser http client")
    });

    let response = fetch(|| BROWSER.get(url.clone()).send())
        .await
        .with_context(|| format!("could not read {url}"))?;

    Ok(response.text().await?)
}

async fn get(session: &Session, url: &Url, headers: &[(&str, &str)]) -> Result<Vec<u8>> {
    let bytes = fetch(|| async {
        let mut request = Request::builder()
            .method(Method::GET)
            .uri(url.as_str())
            .header(header::ACCEPT, "*/*");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        session
            .http_client()
            .request_body(request.body(Default::default())?)
            .await
    })
    .await?;

    Ok(bytes.to_vec())
}
