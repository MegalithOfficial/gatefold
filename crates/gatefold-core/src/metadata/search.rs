use anyhow::{Context, Result, bail};
use librespot::core::Session;
use serde::Deserialize;
use url::Url;

use crate::{
    model::{
        ArtistRef, SearchAlbum, SearchArtist, SearchAudiobook, SearchEpisode, SearchOptions,
        SearchOwner, SearchPage, SearchPlaylist, SearchResults, SearchShow, SearchTrack,
    },
    net,
};

const ENDPOINT: &str = "https://api.spotify.com/v1/search";

pub async fn search(
    session: &Session,
    query: &str,
    options: &SearchOptions,
) -> Result<SearchResults> {
    if query.trim().is_empty() {
        bail!("search query cannot be empty");
    }
    if options.types.is_empty() {
        bail!("search must request at least one result type");
    }
    if options.limit > 10 {
        bail!("search limit cannot exceed 10");
    }
    if options.offset > 1000 {
        bail!("search offset cannot exceed 1000");
    }

    let mut url = Url::parse(ENDPOINT)?;
    let types = options
        .types
        .iter()
        .map(|kind| kind.as_str())
        .collect::<Vec<_>>()
        .join(",");
    {
        let mut params = url.query_pairs_mut();
        params.append_pair("q", query.trim());
        params.append_pair("type", &types);
        params.append_pair("limit", &options.limit.to_string());
        params.append_pair("offset", &options.offset.to_string());
        if let Some(market) = options.market.as_deref() {
            params.append_pair("market", market);
        }
        if options.include_external_audio {
            params.append_pair("include_external", "audio");
        }
    }

    let bytes = net::web_api(session, &url).await?;

    let response: ApiResults =
        serde_json::from_slice(&bytes).context("Spotify returned malformed search results")?;
    Ok(response.into())
}

#[derive(Deserialize)]
struct ApiResults {
    albums: Option<ApiPage<ApiAlbum>>,
    artists: Option<ApiPage<ApiArtist>>,
    playlists: Option<ApiPage<Option<ApiPlaylist>>>,
    tracks: Option<ApiPage<ApiTrack>>,
    shows: Option<ApiPage<ApiShow>>,
    episodes: Option<ApiPage<ApiEpisode>>,
    audiobooks: Option<ApiPage<ApiAudiobook>>,
}

#[derive(Deserialize)]
struct ApiPage<T> {
    href: String,
    items: Vec<T>,
    total: u32,
    limit: u32,
    offset: u32,
    next: Option<String>,
    previous: Option<String>,
}

impl<T, U> From<ApiPage<T>> for SearchPage<U>
where
    T: Into<U>,
{
    fn from(page: ApiPage<T>) -> Self {
        Self {
            href: page.href,
            items: page.items.into_iter().map(Into::into).collect(),
            total: page.total,
            limit: page.limit,
            offset: page.offset,
            next: page.next,
            previous: page.previous,
        }
    }
}

fn optional_page<T, U>(page: ApiPage<Option<T>>) -> SearchPage<U>
where
    T: Into<U>,
{
    SearchPage {
        href: page.href,
        items: page.items.into_iter().flatten().map(Into::into).collect(),
        total: page.total,
        limit: page.limit,
        offset: page.offset,
        next: page.next,
        previous: page.previous,
    }
}

impl From<ApiResults> for SearchResults {
    fn from(results: ApiResults) -> Self {
        Self {
            albums: results.albums.map(Into::into),
            artists: results.artists.map(Into::into),
            playlists: results.playlists.map(optional_page),
            tracks: results.tracks.map(Into::into),
            shows: results.shows.map(Into::into),
            episodes: results.episodes.map(Into::into),
            audiobooks: results.audiobooks.map(Into::into),
        }
    }
}

#[derive(Deserialize)]
struct ApiImage {
    url: String,
    width: Option<u32>,
    height: Option<u32>,
}

fn largest_image(images: Vec<ApiImage>) -> Option<String> {
    images
        .into_iter()
        .max_by_key(|image| image.width.unwrap_or(0) as u64 * image.height.unwrap_or(0) as u64)
        .map(|image| image.url)
}

#[derive(Deserialize)]
struct ApiArtistRef {
    uri: String,
    name: String,
}

impl From<ApiArtistRef> for ArtistRef {
    fn from(artist: ApiArtistRef) -> Self {
        Self {
            uri: artist.uri,
            name: artist.name,
        }
    }
}

#[derive(Deserialize)]
struct ApiAlbum {
    uri: String,
    name: String,
    #[serde(default)]
    artists: Vec<ApiArtistRef>,
    #[serde(default)]
    images: Vec<ApiImage>,
    #[serde(default)]
    release_date: String,
    total_tracks: u32,
    #[serde(default)]
    album_type: String,
}

impl From<ApiAlbum> for SearchAlbum {
    fn from(album: ApiAlbum) -> Self {
        Self {
            uri: album.uri,
            name: album.name,
            artists: album.artists.into_iter().map(Into::into).collect(),
            cover: largest_image(album.images),
            release_date: album.release_date,
            total_tracks: album.total_tracks,
            album_type: album.album_type,
        }
    }
}

#[derive(Deserialize)]
struct ApiFollowers {
    total: u64,
}

#[derive(Deserialize)]
struct ApiArtist {
    uri: String,
    name: String,
    #[serde(default)]
    images: Vec<ApiImage>,
    followers: Option<ApiFollowers>,
    #[serde(default)]
    popularity: u32,
    #[serde(default)]
    genres: Vec<String>,
}

impl From<ApiArtist> for SearchArtist {
    fn from(artist: ApiArtist) -> Self {
        Self {
            uri: artist.uri,
            name: artist.name,
            portrait: largest_image(artist.images),
            followers: artist
                .followers
                .map(|followers| followers.total)
                .unwrap_or(0),
            popularity: artist.popularity,
            genres: artist.genres,
        }
    }
}

#[derive(Deserialize)]
struct ApiOwner {
    uri: String,
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct ApiTotal {
    total: u32,
}

#[derive(Deserialize)]
struct ApiPlaylist {
    uri: String,
    name: String,
    description: Option<String>,
    owner: ApiOwner,
    #[serde(default)]
    images: Vec<ApiImage>,
    items: Option<ApiTotal>,
    tracks: Option<ApiTotal>,
    #[serde(default)]
    collaborative: bool,
    public: Option<bool>,
}

impl From<ApiPlaylist> for SearchPlaylist {
    fn from(playlist: ApiPlaylist) -> Self {
        Self {
            uri: playlist.uri,
            name: playlist.name,
            description: playlist.description.unwrap_or_default(),
            owner: SearchOwner {
                uri: playlist.owner.uri,
                name: playlist.owner.display_name.unwrap_or_default(),
            },
            picture: largest_image(playlist.images),
            total_tracks: playlist
                .items
                .or(playlist.tracks)
                .map(|items| items.total)
                .unwrap_or_default(),
            collaborative: playlist.collaborative,
            public: playlist.public,
        }
    }
}

#[derive(Deserialize)]
struct ApiTrack {
    uri: String,
    name: String,
    #[serde(default)]
    artists: Vec<ApiArtistRef>,
    album: ApiAlbum,
    duration_ms: u32,
    #[serde(default)]
    explicit: bool,
    is_playable: Option<bool>,
    #[serde(default)]
    popularity: u32,
    #[serde(default)]
    disc_number: u32,
    #[serde(default)]
    track_number: u32,
}

impl From<ApiTrack> for SearchTrack {
    fn from(track: ApiTrack) -> Self {
        Self {
            uri: track.uri,
            name: track.name,
            artists: track.artists.into_iter().map(Into::into).collect(),
            album: track.album.into(),
            duration_ms: track.duration_ms,
            is_explicit: track.explicit,
            is_playable: track.is_playable,
            popularity: track.popularity,
            disc: track.disc_number,
            number: track.track_number,
        }
    }
}

#[derive(Deserialize)]
struct ApiShow {
    uri: String,
    name: String,
    #[serde(default)]
    publisher: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    images: Vec<ApiImage>,
    #[serde(default)]
    total_episodes: u32,
    #[serde(default)]
    explicit: bool,
    #[serde(default)]
    media_type: String,
    #[serde(default)]
    languages: Vec<String>,
}

impl From<ApiShow> for SearchShow {
    fn from(show: ApiShow) -> Self {
        Self {
            uri: show.uri,
            name: show.name,
            publisher: show.publisher,
            description: show.description,
            picture: largest_image(show.images),
            total_episodes: show.total_episodes,
            is_explicit: show.explicit,
            media_type: show.media_type,
            languages: show.languages,
        }
    }
}

#[derive(Deserialize)]
struct ApiEpisode {
    uri: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    images: Vec<ApiImage>,
    duration_ms: u32,
    #[serde(default)]
    release_date: String,
    #[serde(default)]
    explicit: bool,
    is_playable: Option<bool>,
    #[serde(default)]
    languages: Vec<String>,
    audio_preview_url: Option<String>,
}

impl From<ApiEpisode> for SearchEpisode {
    fn from(episode: ApiEpisode) -> Self {
        Self {
            uri: episode.uri,
            name: episode.name,
            description: episode.description,
            picture: largest_image(episode.images),
            duration_ms: episode.duration_ms,
            release_date: episode.release_date,
            is_explicit: episode.explicit,
            is_playable: episode.is_playable,
            languages: episode.languages,
            audio_preview_url: episode.audio_preview_url,
        }
    }
}

#[derive(Deserialize)]
struct ApiName {
    name: String,
}

#[derive(Deserialize)]
struct ApiAudiobook {
    uri: String,
    name: String,
    #[serde(default)]
    authors: Vec<ApiName>,
    #[serde(default)]
    narrators: Vec<ApiName>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    edition: String,
    #[serde(default)]
    images: Vec<ApiImage>,
    #[serde(default)]
    total_chapters: u32,
    #[serde(default)]
    explicit: bool,
    #[serde(default)]
    media_type: String,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default)]
    publisher: String,
}

impl From<ApiAudiobook> for SearchAudiobook {
    fn from(audiobook: ApiAudiobook) -> Self {
        Self {
            uri: audiobook.uri,
            name: audiobook.name,
            authors: audiobook
                .authors
                .into_iter()
                .map(|author| author.name)
                .collect(),
            narrators: audiobook
                .narrators
                .into_iter()
                .map(|narrator| narrator.name)
                .collect(),
            description: audiobook.description,
            edition: audiobook.edition,
            picture: largest_image(audiobook.images),
            total_chapters: audiobook.total_chapters,
            is_explicit: audiobook.explicit,
            media_type: audiobook.media_type,
            languages: audiobook.languages,
            publisher: audiobook.publisher,
        }
    }
}
