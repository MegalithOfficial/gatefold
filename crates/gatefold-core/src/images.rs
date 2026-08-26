use std::path::PathBuf;

use anyhow::{Context, Result};
use librespot::core::{FileId, Session};

use crate::net;

pub async fn fetch(session: &Session, id: &str) -> Result<PathBuf> {
    let raw = decode(id)?;

    let dir = crate::cache_dir()?.join("images");
    let path = dir.join(format!("{id}.jpg"));
    if path.exists() {
        return Ok(path);
    }

    let file = FileId::from_raw(&raw);
    let bytes = net::fetch(|| session.spclient().get_image(&file)).await?;

    std::fs::create_dir_all(&dir)?;
    let staging = path.with_extension("part");
    std::fs::write(&staging, &bytes)?;
    std::fs::rename(&staging, &path)?;

    Ok(path)
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
