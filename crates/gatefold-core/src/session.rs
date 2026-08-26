use anyhow::Result;

use crate::{model::Profile, net};
pub use librespot::core::Session;
use librespot::core::{SessionConfig, authentication::Credentials, cache::Cache};

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

pub async fn profile(session: &Session) -> Result<Profile> {
    let username = session.username();
    let bytes = net::fetch(|| session.spclient().get_user_profile(&username, None, None)).await?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)?;

    let profile = Profile {
        name: json["name"].as_str().unwrap_or(&username).to_owned(),
        avatar: json["image_url"].as_str().map(str::to_owned),
    };

    store(&profile);

    Ok(profile)
}

pub fn cached_profile() -> Option<Profile> {
    let dir = crate::cache_dir().ok()?;
    let json = std::fs::read_to_string(dir.join("profile.json")).ok()?;

    serde_json::from_str(&json).ok()
}

fn store(profile: &Profile) {
    let Ok(dir) = crate::cache_dir() else {
        return;
    };
    let Ok(json) = serde_json::to_string(profile) else {
        return;
    };
    let path = dir.join("profile.json");
    let staging = path.with_extension("part");
    if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&staging, json).is_ok() {
        let _ = std::fs::rename(&staging, &path);
    }
}
