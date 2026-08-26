use anyhow::{Context, Result};
use futures::StreamExt;
use librespot::core::{Session, SpotifyUri};
use librespot::metadata::{Album, Metadata, Track};

use crate::model::{AlbumInfo, TrackInfo};
use crate::net;

const CONCURRENCY: usize = 16;

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

pub async fn album(session: &Session, uri: &str) -> Result<AlbumInfo> {
    let uri = SpotifyUri::from_uri(uri)?;
    let album = net::fetch(|| Album::get(session, &uri)).await?;

    let tracks: Vec<TrackInfo> = futures::stream::iter(album.tracks().cloned())
        .map(|track_uri| {
            let session = session.clone();
            async move { net::fetch(|| Track::get(&session, &track_uri)).await }
        })
        .buffered(CONCURRENCY)
        .filter_map(|track| async move {
            match track {
                Ok(track) => TrackInfo::from_track(&track),
                Err(error) => {
                    tracing::warn!("skipping album track: {error}");
                    None
                }
            }
        })
        .collect()
        .await;

    AlbumInfo::from_album(&album, tracks).context("album has no usable uri")
}
