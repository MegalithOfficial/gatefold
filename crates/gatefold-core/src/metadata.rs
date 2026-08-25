use anyhow::{Context, Result};
use librespot::core::{Session, SpotifyUri};
use librespot::metadata::{Metadata, Track};

pub async fn track(session: &Session, uri: &str) -> Result<Track> {
    Ok(Track::get(session, &SpotifyUri::from_uri(uri)?).await?)
}

pub async fn cover(session: &Session, track: &Track) -> Result<Vec<u8>> {
    let image = track
        .album
        .covers
        .iter()
        .chain(track.album.cover_group.iter())
        .max_by_key(|image| image.width)
        .context("track has no cover art")?;

    Ok(session.spclient().get_image(&image.id).await?.to_vec())
}
