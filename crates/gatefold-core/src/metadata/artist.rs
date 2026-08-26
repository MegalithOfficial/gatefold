use anyhow::{Context, Result};
use librespot::core::{Session, SpotifyUri};
use librespot::metadata::artist::AlbumGroups;
use librespot::metadata::{Artist, Metadata};

use crate::model::ArtistInfo;
use crate::net;

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

fn group_heads(groups: &AlbumGroups) -> Vec<SpotifyUri> {
    groups
        .iter()
        .filter_map(|group| group.first().cloned())
        .collect()
}
