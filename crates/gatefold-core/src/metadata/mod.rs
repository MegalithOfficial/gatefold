mod album;
mod artist;
mod catalog;
mod discography;
mod pathfinder;
mod playlist;
mod queries;
mod search;
mod track;

use std::collections::{HashMap, HashSet};

pub use album::album;
pub use artist::{artist, artist_albums};
pub use discography::{artist_track_page, artist_tracks, discography, discography_page};
pub use playlist::{cached_playlists, playlist, playlist_uris, playlists};
pub use search::search;
pub use track::{cover, track};

use futures::StreamExt;
use librespot::{
    core::{Session, SpotifyUri},
    metadata::{Album, Track},
    protocol::{
        extended_metadata::{BatchedEntityRequest, EntityRequest, ExtensionQuery},
        extension_kind::ExtensionKind,
        metadata::{Album as AlbumMessage, Track as TrackMessage},
    },
};
use protobuf::{EnumOrUnknown, Message};

use crate::{
    model::{AlbumRef, TrackInfo},
    net,
};

const BATCH: usize = 50;
const BATCH_CONCURRENCY: usize = 4;

pub(crate) async fn tracks(session: &Session, uris: Vec<SpotifyUri>) -> Vec<TrackInfo> {
    let order = strings(uris);
    let mut tracks = HashMap::with_capacity(order.len());
    for (uri, data) in extended(session, &order, ExtensionKind::TRACK_V4).await {
        let track = match TrackMessage::parse_from_bytes(&data)
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
            tracks.insert(uri, track);
        }
    }

    order
        .into_iter()
        .filter_map(|uri| tracks.get(&uri).cloned())
        .collect()
}

pub(crate) async fn album_refs(session: &Session, uris: Vec<SpotifyUri>) -> Vec<AlbumRef> {
    let order = strings(uris);
    let mut albums = HashMap::with_capacity(order.len());
    for (uri, data) in extended(session, &order, ExtensionKind::ALBUM_V4).await {
        let album = match AlbumMessage::parse_from_bytes(&data)
            .map_err(Into::into)
            .and_then(|message| Album::try_from(&message))
        {
            Ok(album) => album,
            Err(error) => {
                tracing::warn!("skipping album metadata: {error}");
                continue;
            }
        };
        if let Some(album) = AlbumRef::from_album(&album) {
            albums.insert(uri, album);
        }
    }

    order
        .into_iter()
        .filter_map(|uri| albums.get(&uri).cloned())
        .collect()
}

pub(crate) async fn album_tracks(session: &Session, uris: Vec<SpotifyUri>) -> Vec<TrackInfo> {
    let order = strings(uris);
    let mut albums = HashMap::with_capacity(order.len());
    for (uri, data) in extended(session, &order, ExtensionKind::ALBUM_V4).await {
        let album = match AlbumMessage::parse_from_bytes(&data)
            .map_err(Into::into)
            .and_then(|message| Album::try_from(&message))
        {
            Ok(album) => album,
            Err(error) => {
                tracing::warn!("skipping album metadata: {error}");
                continue;
            }
        };
        albums.insert(uri, album.tracks().cloned().collect::<Vec<_>>());
    }

    // The batch answers in its own order; releases keep the order they were asked in.
    let mut seen = HashSet::new();
    let tracks = order
        .iter()
        .filter_map(|uri| albums.get(uri))
        .flatten()
        .filter(|track| track.to_uri().ok().is_some_and(|uri| seen.insert(uri)))
        .cloned()
        .collect();

    self::tracks(session, tracks).await
}

fn strings(uris: Vec<SpotifyUri>) -> Vec<String> {
    uris.into_iter()
        .filter_map(|uri| uri.to_uri().ok())
        .collect()
}

async fn extended(
    session: &Session,
    uris: &[String],
    kind: ExtensionKind,
) -> Vec<(String, Vec<u8>)> {
    let mut seen = HashSet::new();
    let unique: Vec<&String> = uris
        .iter()
        .filter(|uri| seen.insert(uri.as_str()))
        .collect();
    let requests = unique
        .chunks(BATCH)
        .map(|batch| BatchedEntityRequest {
            entity_request: batch
                .iter()
                .map(|uri| EntityRequest {
                    entity_uri: (*uri).clone(),
                    query: vec![ExtensionQuery {
                        extension_kind: EnumOrUnknown::new(kind),
                        ..Default::default()
                    }],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        })
        .collect::<Vec<_>>();

    futures::stream::iter(requests)
        .map(|request| async move {
            let response = match net::fetch(|| {
                session.spclient().get_extended_metadata(request.clone())
            })
            .await
            {
                Ok(response) => response,
                Err(error) => {
                    tracing::warn!("skipping metadata batch: {error}");
                    return Vec::new();
                }
            };

            let mut found = Vec::new();
            for mut entry in response
                .extended_metadata
                .into_iter()
                .flat_map(|metadata| metadata.extension_data)
            {
                match entry.extension_data.take() {
                    Some(data) => found.push((entry.entity_uri, data.value)),
                    None => tracing::warn!("skipping metadata without data: {}", entry.entity_uri),
                }
            }
            found
        })
        .buffer_unordered(BATCH_CONCURRENCY)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect()
}
