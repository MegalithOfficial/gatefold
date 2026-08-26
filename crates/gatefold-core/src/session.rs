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
    let profile = user_profile(session, &username).await?;

    store(&profile);

    Ok(profile)
}

pub async fn user_profile(session: &Session, username: &str) -> Result<Profile> {
    let bytes = net::fetch(|| session.spclient().get_user_profile(username, None, None)).await?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)?;

    Ok(Profile {
        name: json["name"].as_str().unwrap_or(username).to_owned(),
        avatar: json["image_url"].as_str().map(str::to_owned),
    })
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

pub async fn display_name(session: &Session, username: &str) -> Result<String> {
    if let Some(name) = cached_display_name(session, username) {
        return Ok(name);
    }

    let name = user_profile(session, username).await?.name;

    if let Ok(dir) = crate::cache_dir() {
        let path = dir.join("names.json");
        let mut names: std::collections::HashMap<String, String> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        names.insert(username.to_owned(), name.clone());
        if let Ok(json) = serde_json::to_string(&names) {
            let staging = path.with_extension("part");
            if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&staging, json).is_ok() {
                let _ = std::fs::rename(&staging, &path);
            }
        }
    }

    Ok(name)
}

pub fn cached_display_name(session: &Session, username: &str) -> Option<String> {
    if username == session.username()
        && let Some(profile) = cached_profile()
    {
        return Some(profile.name);
    }

    let dir = crate::cache_dir().ok()?;
    let json = std::fs::read_to_string(dir.join("names.json")).ok()?;
    let names: std::collections::HashMap<String, String> = serde_json::from_str(&json).ok()?;

    names.get(username).cloned()
}
