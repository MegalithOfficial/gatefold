use anyhow::{Context, Result};
use librespot::{
    core::{Session, SpotifyUri},
    metadata::{Metadata, Track},
};

use crate::net;

pub async fn track(session: &Session, uri: &str) -> Result<Track> {
    let uri = SpotifyUri::from_uri(uri)?;

    Ok(net::fetch(|| Track::get(session, &uri)).await?)
}

pub async fn cover(session: &Session, track: &Track) -> Result<Vec<u8>> {
    let image = track
        .album
        .covers
        .iter()
        .chain(track.album.cover_group.iter())
        .max_by_key(|image| image.width)
        .context("track has no cover art")?;

    Ok(net::fetch(|| session.spclient().get_image(&image.id))
        .await?
        .to_vec())
}
