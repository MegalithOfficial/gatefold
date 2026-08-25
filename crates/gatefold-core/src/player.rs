use std::sync::Arc;

use anyhow::{Context, Result};
use librespot::core::{Session, SpotifyUri};
use librespot::playback::audio_backend;
use librespot::playback::config::{AudioFormat, PlayerConfig};
use librespot::playback::mixer::NoOpVolume;
pub use librespot::playback::player::Player;

pub fn start(session: Session) -> Result<Arc<Player>> {
    let backend = audio_backend::find(None).context("no audio backend")?;

    Ok(Player::new(
        PlayerConfig::default(),
        session,
        Box::new(NoOpVolume),
        move || backend(None, AudioFormat::default()),
    ))
}

pub fn load(player: &Player, uri: &str) -> Result<()> {
    player.load(SpotifyUri::from_uri(uri)?, true, 0);

    Ok(())
}
