use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::PathBuf,
};

use anyhow::{Context, Result};
use librespot::core::{FileId, Session};

use crate::net;

pub fn cached(picture: &str) -> Option<PathBuf> {
    let path = crate::cache_dir()
        .ok()?
        .join("images")
        .join(format!("{}.jpg", key(picture)));
    path.exists().then_some(path)
}

pub async fn fetch(session: &Session, picture: &str) -> Result<PathBuf> {
    let dir = crate::cache_dir()?.join("images");
    let path = dir.join(format!("{}.jpg", key(picture)));
    if path.exists() {
        return Ok(path);
    }

    let bytes = if picture.starts_with("http") {
        net::fetch(|| session.spclient().request_url(picture)).await?
    } else {
        let file = FileId::from_raw(&decode(picture)?);
        net::fetch(|| session.spclient().get_image(&file)).await?
    };

    std::fs::create_dir_all(&dir)?;
    let staging = path.with_extension("part");
    std::fs::write(&staging, &bytes)?;
    std::fs::rename(&staging, &path)?;

    Ok(path)
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
