use anyhow::{Context, Result};
use librespot::{
    core::{Session, SpotifyUri},
    metadata::{Metadata, playlist::Playlist},
    protocol::{playlist4_external, playlist4_external::ListAttributes as PlaylistAttributes},
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
            let attributes = meta.map(|meta| meta.attributes.get_or_default());
            refs.push(PlaylistRef {
                uri: uri.to_owned(),
                name: attributes.map(|a| a.name().to_owned()).unwrap_or_default(),
                owner: meta
                    .map(|meta| meta.owner_username().to_owned())
                    .unwrap_or_default(),
                length: meta
                    .map(|meta| meta.length().max(0) as usize)
                    .unwrap_or_default(),
                picture: attributes.and_then(picture),
            });
        }

        from += count;
        if count < PAGE || from >= message.length().max(0) as usize {
            break;
        }
    }

    store(&refs);

    Ok(refs)
}

fn picture(attributes: &PlaylistAttributes) -> Option<String> {
    let raw = attributes.picture();
    if !raw.is_empty() {
        return Some(raw.iter().map(|byte| format!("{byte:02x}")).collect());
    }

    attributes
        .picture_size
        .iter()
        .find(|size| size.target_name() == "default")
        .or_else(|| attributes.picture_size.first())
        .map(|size| size.url().to_owned())
}

pub fn cached_playlists() -> Vec<PlaylistRef> {
    let Ok(dir) = crate::cache_dir() else {
        return Vec::new();
    };
    std::fs::read_to_string(dir.join("library.json"))
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn store(refs: &[PlaylistRef]) {
    let Ok(dir) = crate::cache_dir() else {
        return;
    };
    let Ok(json) = serde_json::to_string(refs) else {
        return;
    };
    let path = dir.join("library.json");
    let staging = path.with_extension("part");
    if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&staging, json).is_ok() {
        let _ = std::fs::rename(&staging, &path);
    }
}

pub async fn playlist(session: &Session, uri: &str) -> Result<PlaylistInfo> {
    let id = SpotifyUri::from_uri(uri)?;
    let playlist = net::fetch(|| Playlist::get(session, &id)).await?;

    let uris: Vec<SpotifyUri> = playlist.tracks().cloned().collect();
    let owner_username = match &playlist.id {
        SpotifyUri::Playlist {
            user: Some(user), ..
        } => user.clone(),
        _ => String::new(),
    };
    let tracks = super::track_batch(session, uris).await;

    Ok(PlaylistInfo {
        uri: playlist.id.to_uri().context("playlist has no usable uri")?,
        name: playlist.attributes.name.clone(),
        description: description_text(&playlist.attributes.description),
        owner: owner_username,
        updated_at_ms: playlist.timestamp.as_timestamp_ms(),
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

fn description_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut tag = String::new();
    let mut in_tag = false;

    for character in html.chars() {
        match character {
            '<' if !in_tag => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                if matches!(
                    tag.trim().to_ascii_lowercase().as_str(),
                    "br" | "br/" | "/p"
                ) {
                    text.push(' ');
                }
                in_tag = false;
            }
            _ if in_tag => tag.push(character),
            _ => text.push(character),
        }
    }

    decode_entities(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        let entity = &rest[start + 1..];
        let Some(end) = entity.find(';').filter(|end| *end <= 8) else {
            decoded.push('&');
            rest = entity;
            continue;
        };
        let name = &entity[..end];
        if let Some(character) = decode_entity(name) {
            decoded.push(character);
        } else {
            decoded.push('&');
            decoded.push_str(name);
            decoded.push(';');
        }
        rest = &entity[end + 1..];
    }

    decoded.push_str(rest);
    decoded
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "apos" | "#39" => Some('\''),
        "gt" => Some('>'),
        "hellip" => Some('…'),
        "lt" => Some('<'),
        "nbsp" => Some(' '),
        "quot" => Some('"'),
        value if value.starts_with("#x") || value.starts_with("#X") => {
            u32::from_str_radix(&value[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        value if value.starts_with('#') => value[1..].parse().ok().and_then(char::from_u32),
        _ => None,
    }
}
