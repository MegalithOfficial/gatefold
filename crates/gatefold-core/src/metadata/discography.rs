use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock, Mutex},
};

use anyhow::Result;
use librespot::core::Session;

use crate::model::{AlbumRef, ReleaseGroup, TrackInfo};

const PAGE: usize = 50;

type ReleaseCell = Arc<tokio::sync::OnceCell<Arc<Vec<AlbumRef>>>>;
type TrackCell = Arc<tokio::sync::OnceCell<Arc<Vec<TrackInfo>>>>;

static RELEASES: LazyLock<Mutex<HashMap<(String, ReleaseGroup), ReleaseCell>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static TRACKS: LazyLock<Mutex<HashMap<String, TrackCell>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub async fn discography(
    session: &Session,
    uri: &str,
    group: ReleaseGroup,
) -> Result<Vec<AlbumRef>> {
    Ok(releases(session, uri, group).await?.as_ref().clone())
}

pub async fn discography_page(
    session: &Session,
    uri: &str,
    group: ReleaseGroup,
    offset: Option<usize>,
) -> Result<(Vec<AlbumRef>, usize, Option<usize>)> {
    let releases = releases(session, uri, group).await?;
    let offset = offset.unwrap_or_default().min(releases.len());
    let end = (offset + PAGE).min(releases.len());
    let next = (end < releases.len()).then_some(end);

    Ok((releases[offset..end].to_vec(), releases.len(), next))
}

pub async fn artist_tracks(session: &Session, uri: &str) -> Result<Vec<TrackInfo>> {
    Ok(tracks(session, uri).await?.as_ref().clone())
}

pub async fn artist_track_page(
    session: &Session,
    uri: &str,
    offset: Option<usize>,
) -> Result<(Vec<TrackInfo>, Option<usize>)> {
    let tracks = tracks(session, uri).await?;
    let offset = offset.unwrap_or_default().min(tracks.len());
    let end = (offset + PAGE).min(tracks.len());
    let next = (end < tracks.len()).then_some(end);

    Ok((tracks[offset..end].to_vec(), next))
}

async fn releases(session: &Session, uri: &str, group: ReleaseGroup) -> Result<Arc<Vec<AlbumRef>>> {
    let key = (uri.to_owned(), group);
    let cell = {
        let mut releases = RELEASES.lock().unwrap_or_else(|error| error.into_inner());
        releases
            .entry(key)
            .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
            .clone()
    };

    cell.get_or_try_init(|| async {
        let artist = super::artist::raw(session, uri).await?;
        let uris = match group {
            ReleaseGroup::Albums => super::artist::group_heads(&artist.albums),
            ReleaseGroup::Singles => super::artist::group_heads(&artist.singles),
            ReleaseGroup::Compilations => super::artist::group_heads(&artist.compilations),
            ReleaseGroup::AppearsOn => super::artist::group_heads(&artist.appears_on_albums),
        };
        Ok::<_, anyhow::Error>(Arc::new(super::album_refs(session, uris).await))
    })
    .await
    .cloned()
}

async fn tracks(session: &Session, uri: &str) -> Result<Arc<Vec<TrackInfo>>> {
    let cell = {
        let mut tracks = TRACKS.lock().unwrap_or_else(|error| error.into_inner());
        tracks
            .entry(uri.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
            .clone()
    };

    cell.get_or_try_init(|| async {
        let artist = super::artist::raw(session, uri).await?;
        let mut albums = super::artist::group_heads(&artist.albums);
        albums.extend(super::artist::group_heads(&artist.singles));
        let tracks = super::album_tracks(session, albums).await;
        let mut seen = HashSet::new();
        Ok::<_, anyhow::Error>(Arc::new(
            tracks
                .into_iter()
                .filter(|track| seen.insert(track.uri.clone()))
                .collect(),
        ))
    })
    .await
    .cloned()
}
