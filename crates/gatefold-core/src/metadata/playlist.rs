use anyhow::{Context, Result};
use librespot::{
    core::{Session, SpotifyUri},
    metadata::{
        Metadata,
        playlist::{Playlist, list::SelectedListContent},
    },
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
        let page = SelectedListContent::try_from(&message)?;

        let items = page.contents.items.0;
        let count = items.len();
        let metas = page.contents.meta_items.0;

        for (index, item) in items.into_iter().enumerate() {
            let SpotifyUri::Playlist { .. } = item.id else {
                continue;
            };
            let Ok(uri) = item.id.to_uri() else {
                continue;
            };

            let meta = metas.get(index);
            refs.push(PlaylistRef {
                uri,
                name: meta
                    .map(|meta| meta.attributes.name.clone())
                    .unwrap_or_default(),
                owner: meta
                    .map(|meta| meta.owner_username.clone())
                    .unwrap_or_default(),
                length: meta
                    .map(|meta| meta.length.max(0) as usize)
                    .unwrap_or_default(),
            });
        }

        from += count;
        if count < PAGE || from >= page.length.max(0) as usize {
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
