use std::path::PathBuf;

use anyhow::{Context, Result};

pub mod auth;
pub mod images;
pub mod local_search;
pub mod lyrics;
pub mod metadata;
pub mod model;
pub mod net;
pub mod player;
pub mod session;
pub mod settings;
mod sink;

pub fn config_dir() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("no config directory")?
        .join("gatefold"))
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(dirs::cache_dir()
        .context("no cache directory")?
        .join("gatefold"))
}
