mod album;
mod artist;
mod playlist;
mod track;

pub use album::album;
pub use artist::artist;
pub use playlist::{playlist, playlist_uris, playlists};
pub use track::{cover, track};

use futures::StreamExt;
use librespot::{
    core::{Session, SpotifyUri},
    metadata::{Album, Metadata, Track},
};

use crate::{
    model::{AlbumRef, TrackInfo},
    net,
};

const CONCURRENCY: usize = 16;

pub(crate) async fn tracks(session: &Session, uris: Vec<SpotifyUri>) -> Vec<TrackInfo> {
    futures::stream::iter(uris)
        .map(|track_uri| {
            let session = session.clone();
            async move { net::fetch(|| Track::get(&session, &track_uri)).await }
        })
        .buffered(CONCURRENCY)
        .filter_map(|track| async move {
            match track {
                Ok(track) => TrackInfo::from_track(&track),
                Err(error) => {
                    tracing::warn!("skipping track: {error}");
                    None
                }
            }
        })
        .collect()
        .await
}

pub(crate) async fn album_refs(session: &Session, uris: Vec<SpotifyUri>) -> Vec<AlbumRef> {
    futures::stream::iter(uris)
        .map(|album_uri| {
            let session = session.clone();
            async move { net::fetch(|| Album::get(&session, &album_uri)).await }
        })
        .buffered(CONCURRENCY)
        .filter_map(|album| async move {
            match album {
                Ok(album) => AlbumRef::from_album(&album),
                Err(error) => {
                    tracing::warn!("skipping album: {error}");
                    None
                }
            }
        })
        .collect()
        .await
}
