mod album;
mod artist;
mod playlist;
mod search;
mod track;

use std::collections::{HashMap, HashSet};

pub use album::album;
pub use artist::artist;
pub use playlist::{cached_playlists, playlist, playlist_uris, playlists};
pub use search::search;
pub use track::{cover, track};

use futures::StreamExt;
use librespot::{
    core::{Session, SpotifyUri},
    metadata::{Album, Metadata, Track},
    protocol::{
        extended_metadata::{BatchedEntityRequest, EntityRequest, ExtensionQuery},
        extension_kind::ExtensionKind,
        metadata::Track as TrackMessage,
    },
};
use protobuf::{EnumOrUnknown, Message};

use crate::{
    model::{AlbumRef, TrackInfo},
    net,
};

const CONCURRENCY: usize = 16;
const TRACK_BATCH: usize = 50;

pub(crate) async fn tracks(session: &Session, uris: Vec<SpotifyUri>) -> Vec<TrackInfo> {
    let order: Vec<String> = uris
        .into_iter()
        .filter_map(|uri| uri.to_uri().ok())
        .collect();
    let mut seen = HashSet::new();
    let unique: Vec<String> = order
        .iter()
        .filter(|uri| seen.insert((*uri).clone()))
        .cloned()
        .collect();
    let mut tracks = HashMap::with_capacity(unique.len());

    for batch in unique.chunks(TRACK_BATCH) {
        let request = BatchedEntityRequest {
            entity_request: batch
                .iter()
                .map(|uri| EntityRequest {
                    entity_uri: uri.clone(),
                    query: vec![ExtensionQuery {
                        extension_kind: EnumOrUnknown::new(ExtensionKind::TRACK_V4),
                        ..Default::default()
                    }],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let response =
            match net::fetch(|| session.spclient().get_extended_metadata(request.clone())).await {
                Ok(response) => response,
                Err(error) => {
                    tracing::warn!("skipping track metadata batch: {error}");
                    continue;
                }
            };

        for mut entry in response
            .extended_metadata
            .into_iter()
            .flat_map(|metadata| metadata.extension_data)
        {
            let Some(data) = entry.extension_data.take() else {
                tracing::warn!("skipping track metadata without data: {}", entry.entity_uri);
                continue;
            };
            let track = match TrackMessage::parse_from_bytes(&data.value)
                .map_err(Into::into)
                .and_then(|message| Track::try_from(&message))
            {
                Ok(track) => track,
                Err(error) => {
                    tracing::warn!("skipping track metadata: {error}");
                    continue;
                }
            };
            if let Some(track) = TrackInfo::from_track(&track) {
                tracks.insert(track.uri.clone(), track);
            }
        }
    }

    order
        .into_iter()
        .filter_map(|uri| tracks.get(&uri).cloned())
        .collect()
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
