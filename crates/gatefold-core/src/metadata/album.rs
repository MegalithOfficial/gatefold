use anyhow::{Context, Result};
use librespot::{
    core::{Session, SpotifyUri},
    metadata::{Album, Metadata},
};

use crate::{model::AlbumInfo, net};

pub async fn album(session: &Session, uri: &str) -> Result<AlbumInfo> {
    let uri = SpotifyUri::from_uri(uri)?;
    let album = net::fetch(|| Album::get(session, &uri)).await?;

    let uris: Vec<SpotifyUri> = album.tracks().cloned().collect();
    let tracks = super::tracks(session, uris).await;

    AlbumInfo::from_album(&album, tracks).context("album has no usable uri")
}
