use std::path::PathBuf;

use anyhow::{Context, Result};

pub mod auth;
pub mod images;
pub mod metadata;
pub mod model;
pub mod net;
pub mod player;
pub mod session;

pub fn cache_dir() -> Result<PathBuf> {
    Ok(dirs::cache_dir()
        .context("no cache directory")?
        .join("gatefold"))
}
