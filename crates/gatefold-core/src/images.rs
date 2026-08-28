use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex},
};

use anyhow::{Context, Result};
use librespot::core::{FileId, Session};

use crate::net;

static IN_FLIGHT: LazyLock<Mutex<HashMap<String, Arc<tokio::sync::watch::Sender<bool>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct Flight {
    key: String,
    done: Arc<tokio::sync::watch::Sender<bool>>,
}

impl Drop for Flight {
    fn drop(&mut self) {
        let mut flights = IN_FLIGHT.lock().unwrap_or_else(|error| error.into_inner());
        if flights
            .get(&self.key)
            .is_some_and(|done| Arc::ptr_eq(done, &self.done))
        {
            flights.remove(&self.key);
        }
        self.done.send_replace(true);
    }
}

pub fn cached(picture: &str) -> Option<PathBuf> {
    let path = crate::cache_dir()
        .ok()?
        .join("images")
        .join(format!("{}.jpg", key(picture)));
    path.exists().then_some(path)
}

pub async fn fetch(session: &Session, picture: &str) -> Result<PathBuf> {
    let dir = crate::cache_dir()?.join("images");
    let cache_key = key(picture);
    let path = dir.join(format!("{cache_key}.jpg"));

    loop {
        if path.exists() {
            return Ok(path);
        }

        let (mut done, leader) = {
            let mut flights = IN_FLIGHT.lock().unwrap_or_else(|error| error.into_inner());
            match flights.get(&cache_key) {
                Some(done) => (done.subscribe(), false),
                None => {
                    let (done, receiver) = tokio::sync::watch::channel(false);
                    flights.insert(cache_key.clone(), Arc::new(done));
                    (receiver, true)
                }
            }
        };

        if !leader {
            let _ = done.changed().await;
            continue;
        }

        let done = IN_FLIGHT
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&cache_key)
            .cloned()
            .expect("image flight was just registered");
        let _flight = Flight {
            key: cache_key.clone(),
            done,
        };
        return download(session, picture, &dir, &path).await;
    }
}

async fn download(
    session: &Session,
    picture: &str,
    dir: &std::path::Path,
    path: &std::path::Path,
) -> Result<PathBuf> {
    let bytes = if picture.starts_with("http") {
        net::fetch(|| session.spclient().request_url(picture)).await?
    } else {
        let file = FileId::from_raw(&decode(picture)?);
        net::fetch(|| session.spclient().get_image(&file)).await?
    };

    std::fs::create_dir_all(dir)?;
    let staging = path.with_extension("part");
    std::fs::write(&staging, &bytes)?;
    std::fs::rename(&staging, path)?;

    Ok(path.to_owned())
}

fn key(picture: &str) -> String {
    if !picture.starts_with("http") {
        return picture.to_owned();
    }

    let mut hasher = DefaultHasher::new();
    picture.hash(&mut hasher);
    format!("url-{:016x}", hasher.finish())
}

fn decode(hex: &str) -> Result<Vec<u8>> {
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            let pair = hex.get(i..i + 2).context("malformed image id")?;
            u8::from_str_radix(pair, 16).context("malformed image id")
        })
        .collect()
}
