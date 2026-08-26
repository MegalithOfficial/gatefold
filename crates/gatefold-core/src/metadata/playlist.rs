use anyhow::{Context, Result};
use librespot::{
    core::{Session, SpotifyUri},
    metadata::{Metadata, playlist::Playlist},
    protocol::playlist4_external,
};

use protobuf::Message;

use crate::{
    model::{PlaylistInfo, PlaylistRef},
    net,
};

const PAGE: usize = 120;

pub async fn playlists(session: &Session) -> Result<Vec<PlaylistRef>> {
    let mut refs = Vec::new();
    let mut from = 0;

    loop {
        let bytes = net::fetch(|| session.spclient().get_rootlist(from, Some(PAGE))).await?;
        let message = playlist4_external::SelectedListContent::parse_from_bytes(&bytes)?;
        let contents = message.contents.get_or_default();
        let count = contents.items.len();

        for (index, item) in contents.items.iter().enumerate() {
            let uri = item.uri();
            let Ok(SpotifyUri::Playlist { .. }) = SpotifyUri::from_uri(uri) else {
                continue;
            };

            let meta = contents.meta_items.get(index);
            refs.push(PlaylistRef {
                uri: uri.to_owned(),
                name: meta
                    .map(|meta| meta.attributes.get_or_default().name().to_owned())
                    .unwrap_or_default(),
                owner: meta
                    .map(|meta| meta.owner_username().to_owned())
                    .unwrap_or_default(),
                length: meta
                    .map(|meta| meta.length().max(0) as usize)
                    .unwrap_or_default(),
            });
        }

        from += count;
        if count < PAGE || from >= message.length().max(0) as usize {
            break;
        }
    }

    Ok(refs)
}

pub async fn playlist(session: &Session, uri: &str) -> Result<PlaylistInfo> {
    let id = SpotifyUri::from_uri(uri)?;
    let playlist = net::fetch(|| Playlist::get(session, &id)).await?;

    let uris: Vec<SpotifyUri> = playlist.tracks().cloned().collect();
    let tracks = super::tracks(session, uris).await;

    let owner = match &playlist.id {
        SpotifyUri::Playlist {
            user: Some(user), ..
        } => user.clone(),
        _ => String::new(),
    };

    Ok(PlaylistInfo {
        uri: playlist.id.to_uri().context("playlist has no usable uri")?,
        name: playlist.attributes.name.clone(),
        description: playlist.attributes.description.clone(),
        owner,
        tracks,
    })
}

pub async fn playlist_uris(session: &Session, uri: &str) -> Result<Vec<String>> {
    let id = SpotifyUri::from_uri(uri)?;
    let playlist = net::fetch(|| Playlist::get(session, &id)).await?;

    Ok(playlist
        .tracks()
        .filter_map(|track| track.to_uri().ok())
        .collect())
}
