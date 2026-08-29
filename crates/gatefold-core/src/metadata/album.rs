use anyhow::{Context, Result};
use librespot::{
    core::{Session, SpotifyUri},
    metadata::{Album, Metadata},
};

use crate::{model::AlbumInfo, net};

pub async fn album(session: &Session, uri: &str) -> Result<AlbumInfo> {
    let (album, plays) = tokio::join!(
        super::catalog::album(session, uri),
        super::pathfinder::album_plays(session, uri),
    );
    match album {
        Ok(mut album) => {
            match plays {
                Ok(plays) => {
                    for track in &mut album.tracks {
                        track.plays = plays.get(&track.uri).copied();
                    }
                }
                Err(error) => tracing::warn!("album play counts unavailable: {error}"),
            }
            return Ok(album);
        }
        Err(error) => tracing::warn!("Web API album unavailable, using metadata: {error}"),
    }

    fallback(session, uri).await
}

async fn fallback(session: &Session, uri: &str) -> Result<AlbumInfo> {
    let id = SpotifyUri::from_uri(uri)?;
    let album = net::fetch(|| Album::get(session, &id)).await?;

    let uris: Vec<SpotifyUri> = album.tracks().cloned().collect();
    let tracks = super::track_batch(session, uris).await;

    AlbumInfo::from_album(&album, tracks).context("album has no usable uri")
}
