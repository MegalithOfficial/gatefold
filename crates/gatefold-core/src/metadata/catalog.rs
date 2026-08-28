use anyhow::{Context, Result};
use librespot::core::Session;
use serde::Deserialize;
use url::Url;

use crate::{
    model::{AlbumInfo, ArtistRef, TrackInfo},
    net,
};

const API: &str = "https://api.spotify.com/v1";

pub(crate) async fn album(session: &Session, uri: &str) -> Result<AlbumInfo> {
    let id = spotify_id(uri, "album")?;
    let mut url = Url::parse(&format!("{API}/albums/{id}"))?;
    url.query_pairs_mut()
        .append_pair("market", &session.country());
    let mut album: ApiAlbum = serde_json::from_slice(&net::web_api(session, &url).await?)?;
    let cover = largest_image(&album.images);
    let mut tracks = album
        .tracks
        .take()
        .context("album response without tracks")?;
    let mut info_tracks = tracks
        .items
        .drain(..)
        .map(|track| track.info(cover.clone()))
        .collect::<Vec<_>>();

    while let Some(next) = tracks.next.take() {
        let url = Url::parse(&next)?;
        tracks = serde_json::from_slice(&net::web_api(session, &url).await?)?;
        info_tracks.extend(
            tracks
                .items
                .drain(..)
                .map(|track| track.info(cover.clone())),
        );
    }

    Ok(AlbumInfo {
        uri: album.uri,
        name: album.name,
        artists: album.artists.into_iter().map(ArtistRef::from).collect(),
        year: year(&album.release_date),
        label: album.label.unwrap_or_default(),
        copyrights: album.copyrights.into_iter().map(|item| item.text).collect(),
        cover_id: cover,
        tracks: info_tracks,
    })
}

fn spotify_id<'a>(uri: &'a str, kind: &str) -> Result<&'a str> {
    uri.strip_prefix(&format!("spotify:{kind}:"))
        .filter(|id| !id.is_empty() && !id.contains(':'))
        .with_context(|| format!("invalid Spotify {kind} uri"))
}

fn year(date: &str) -> i32 {
    date.get(..4)
        .and_then(|year| year.parse().ok())
        .unwrap_or_default()
}

fn largest_image(images: &[ApiImage]) -> Option<String> {
    images
        .iter()
        .max_by_key(|image| image.width.unwrap_or_default())
        .map(|image| image.url.clone())
}

#[derive(Deserialize)]
struct ApiPage<T> {
    items: Vec<T>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct ApiAlbum {
    uri: String,
    name: String,
    #[serde(default)]
    release_date: String,
    #[serde(default)]
    artists: Vec<ApiArtist>,
    #[serde(default)]
    images: Vec<ApiImage>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    copyrights: Vec<ApiCopyright>,
    #[serde(default)]
    tracks: Option<ApiPage<ApiTrack>>,
}

#[derive(Deserialize)]
struct ApiTrack {
    uri: String,
    name: String,
    #[serde(default)]
    artists: Vec<ApiArtist>,
    #[serde(default)]
    track_number: u32,
    #[serde(default = "one")]
    disc_number: u32,
    #[serde(default)]
    duration_ms: u32,
    #[serde(default)]
    explicit: bool,
}

impl ApiTrack {
    fn info(self, cover_id: Option<String>) -> TrackInfo {
        TrackInfo {
            uri: self.uri,
            name: self.name,
            artists: self.artists.into_iter().map(ArtistRef::from).collect(),
            cover_id,
            number: self.track_number,
            disc: self.disc_number,
            duration_ms: self.duration_ms,
            is_explicit: self.explicit,
            plays: None,
        }
    }
}

#[derive(Deserialize)]
struct ApiArtist {
    uri: String,
    name: String,
}

impl From<ApiArtist> for ArtistRef {
    fn from(artist: ApiArtist) -> Self {
        Self {
            uri: artist.uri,
            name: artist.name,
        }
    }
}

#[derive(Deserialize)]
struct ApiImage {
    url: String,
    width: Option<u32>,
}

#[derive(Deserialize)]
struct ApiCopyright {
    text: String,
}

const fn one() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::{ApiAlbum, largest_image, spotify_id, year};

    #[test]
    fn parses_documented_simplified_album() {
        let album: ApiAlbum = serde_json::from_str(
            r#"{
                "uri":"spotify:album:one",
                "name":"Album",
                "release_date":"2026-08-28",
                "artists":[{"uri":"spotify:artist:one","name":"Artist"}],
                "images":[{"url":"small","width":64},{"url":"large","width":640}]
            }"#,
        )
        .expect("album");
        assert_eq!(year(&album.release_date), 2026);
        assert_eq!(largest_image(&album.images).as_deref(), Some("large"));
    }

    #[test]
    fn extracts_typed_spotify_id() {
        assert_eq!(
            spotify_id("spotify:artist:abc", "artist").expect("id"),
            "abc"
        );
        assert!(spotify_id("spotify:album:abc", "artist").is_err());
    }
}
