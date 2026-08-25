use anyhow::{Context, Result};
use librespot::core::{Session, SpotifyUri};
use librespot::playback::audio_backend;
use librespot::playback::config::{AudioFormat, PlayerConfig};
use librespot::playback::mixer::NoOpVolume;
use librespot::playback::player::Player;

pub async fn play(session: Session, uri: &str) -> Result<()> {
    let backend = audio_backend::find(None).context("no audio backend")?;

    let player = Player::new(
        PlayerConfig::default(),
        session,
        Box::new(NoOpVolume),
        move || backend(None, AudioFormat::default()),
    );

    player.load(SpotifyUri::from_uri(uri)?, true, 0);
    player.await_end_of_track().await;

    Ok(())
}
