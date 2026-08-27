use anyhow::{Context, Result};
use librespot::{
    core::{Session, SpotifyUri},
    metadata::{Artist, Metadata, artist::AlbumGroups},
};

use crate::{
    model::{AlbumRef, ArtistInfo},
    net,
};

pub async fn artist(session: &Session, uri: &str) -> Result<ArtistInfo> {
    let uri = SpotifyUri::from_uri(uri)?;
    let artist = net::fetch(|| Artist::get(session, &uri)).await?;

    let country = session.country();
    let top: Vec<SpotifyUri> = artist
        .top_tracks
        .iter()
        .find(|top| top.country == country)
        .or_else(|| artist.top_tracks.first())
        .map(|top| top.tracks.iter().take(10).cloned().collect())
        .unwrap_or_default();
    let top_tracks = super::tracks(session, top).await;

    let albums = super::album_refs(session, group_heads(&artist.albums)).await;
    let singles = super::album_refs(session, group_heads(&artist.singles)).await;
    let compilations = super::album_refs(session, group_heads(&artist.compilations)).await;

    ArtistInfo::from_artist(&artist, top_tracks, albums, singles, compilations)
        .context("artist has no usable uri")
}

pub async fn artist_albums(session: &Session, uri: &str) -> Result<Vec<AlbumRef>> {
    let uri = SpotifyUri::from_uri(uri)?;
    let artist = net::fetch(|| Artist::get(session, &uri)).await?;

    let mut uris = group_heads(&artist.albums);
    uris.extend(group_heads(&artist.singles));

    Ok(super::album_refs(session, uris).await)
}

fn group_heads(groups: &AlbumGroups) -> Vec<SpotifyUri> {
    groups
        .iter()
        .filter_map(|group| group.first().cloned())
        .collect()
}
