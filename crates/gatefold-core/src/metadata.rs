use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::StreamExt;
use librespot::core::error::ErrorKind;
use librespot::core::{Session, SpotifyUri};
use librespot::metadata::{Album, Metadata, Track};
use tokio::sync::Semaphore;

use crate::model::{AlbumInfo, TrackInfo};

const CONCURRENCY: usize = 32;
const RETRIES: u32 = 3;

static FETCHES: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(CONCURRENCY));

async fn fetch_track(session: &Session, uri: &SpotifyUri) -> Result<Track> {
    let _permit = FETCHES.acquire().await?;

    let mut attempt = 0;
    loop {
        match Track::get(session, uri).await {
            Ok(track) => return Ok(track),
            Err(error) if attempt < RETRIES && retryable(error.kind) => {
                attempt += 1;
                let wait = Duration::from_millis(500 * 4u64.pow(attempt - 1));
                tracing::warn!("track fetch limited ({error}), retrying in {wait:?}");
                tokio::time::sleep(wait).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn retryable(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::ResourceExhausted | ErrorKind::Unavailable | ErrorKind::DeadlineExceeded
    )
}

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
pub async fn album(session: &Session, uri: &str) -> Result<AlbumInfo> {
    let album = Album::get(session, &SpotifyUri::from_uri(uri)?).await?;

    let tracks: Vec<TrackInfo> = futures::stream::iter(album.tracks().cloned())
        .map(|track_uri| {
            let session = session.clone();
            async move { fetch_track(&session, &track_uri).await }
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
