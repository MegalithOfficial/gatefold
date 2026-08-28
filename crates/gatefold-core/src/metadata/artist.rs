use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

use anyhow::{Context, Result};
use librespot::{
    core::{Session, SpotifyUri},
    metadata::{Artist, Metadata, artist::AlbumGroups},
};

use crate::{
    model::{AlbumRef, ArtistInfo},
    net,
};

type ArtistCell = Arc<tokio::sync::OnceCell<Arc<Artist>>>;

static ARTISTS: LazyLock<Mutex<HashMap<String, ArtistCell>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub async fn artist(session: &Session, uri: &str) -> Result<ArtistInfo> {
    match super::pathfinder::artist(session, uri).await {
        Ok(info) => Ok(info),
        Err(error) => {
            tracing::warn!("artist overview unavailable, using metadata: {error}");
            fallback(session, uri).await
        }
    }
}

async fn fallback(session: &Session, uri: &str) -> Result<ArtistInfo> {
    let artist = raw(session, uri).await?;

    let country = session.country();
    let top: Vec<SpotifyUri> = artist
        .top_tracks
        .iter()
        .find(|top| top.country == country)
        .or_else(|| artist.top_tracks.first())
        .map(|top| top.tracks.iter().take(10).cloned().collect())
        .unwrap_or_default();
    let album_uris = group_heads(&artist.albums);
    let single_uris = group_heads(&artist.singles);
    let compilation_uris = group_heads(&artist.compilations);
    let mut all = album_uris.clone();
    all.extend(single_uris.iter().cloned());
    all.extend(compilation_uris.iter().cloned());
    let (top_tracks, refs) =
        tokio::join!(super::tracks(session, top), super::album_refs(session, all));
    let refs = refs
        .into_iter()
        .map(|album| (album.uri.clone(), album))
        .collect::<HashMap<_, _>>();
    let select = |uris: Vec<SpotifyUri>| {
        uris.into_iter()
            .filter_map(|uri| uri.to_uri().ok())
            .filter_map(|uri| refs.get(&uri).cloned())
            .collect()
    };
    let albums = select(album_uris);
    let singles = select(single_uris);
    let compilations = select(compilation_uris);

    ArtistInfo::from_artist(&artist, top_tracks, albums, singles, compilations)
        .context("artist has no usable uri")
}

pub async fn artist_albums(session: &Session, uri: &str) -> Result<Vec<AlbumRef>> {
    let artist = raw(session, uri).await?;

    let mut uris = group_heads(&artist.albums);
    uris.extend(group_heads(&artist.singles));

    Ok(super::album_refs(session, uris).await)
}

pub(crate) async fn raw(session: &Session, uri: &str) -> Result<Arc<Artist>> {
    let id = SpotifyUri::from_uri(uri)?;
    let cell = {
        let mut artists = ARTISTS.lock().unwrap_or_else(|error| error.into_inner());
        artists
            .entry(uri.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
            .clone()
    };

    Ok(cell
        .get_or_try_init(|| async { net::fetch(|| Artist::get(session, &id)).await.map(Arc::new) })
        .await?
        .clone())
}

pub(crate) fn group_heads(groups: &AlbumGroups) -> Vec<SpotifyUri> {
    groups
        .iter()
        .filter_map(|group| group.first().cloned())
        .collect()
}
