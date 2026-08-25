use anyhow::Result;
use librespot::core::authentication::Credentials;
use librespot::core::cache::Cache;
use librespot::core::{Session, SessionConfig};

use crate::auth;

pub async fn connect() -> Result<Session> {
    let cache = cache()?;

    let credentials = match cache.credentials() {
        Some(credentials) => credentials,
        None => Credentials::with_access_token(auth::login().await?.access_token),
    };

    let session = Session::new(SessionConfig::default(), Some(cache));
    session.connect(credentials, true).await?;

    Ok(session)
}

fn cache() -> Result<Cache> {
    let dir = crate::cache_dir()?;

    Ok(Cache::new(Some(&dir), None, None, None)?)
}
