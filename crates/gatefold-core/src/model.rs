use librespot::{
    core::date::Date,
    metadata::{Album, Artist, Track, artist::ArtistsWithRole, image::Images},
};

#[derive(Debug, Clone)]
pub struct ArtistRef {
    pub uri: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub uri: String,
    pub name: String,
    pub artists: Vec<ArtistRef>,
    pub cover_id: Option<String>,
    pub number: u32,
    pub disc: u32,
    pub duration_ms: u32,
    pub is_explicit: bool,
}

#[derive(Debug, Clone)]
pub struct AlbumRef {
    pub uri: String,
    pub name: String,
    pub year: i32,
    pub cover_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArtistInfo {
    pub uri: String,
    pub name: String,
    pub portrait_id: Option<String>,
    pub biography: Option<String>,
    pub top_tracks: Vec<TrackInfo>,
    pub albums: Vec<AlbumRef>,
    pub singles: Vec<AlbumRef>,
    pub compilations: Vec<AlbumRef>,
    pub related: Vec<ArtistRef>,
}

#[derive(Debug, Clone)]
pub struct AlbumInfo {
    pub uri: String,
    pub name: String,
    pub artists: Vec<ArtistRef>,
    pub year: i32,
    pub label: String,
    pub copyrights: Vec<String>,
    pub cover_id: Option<String>,
    pub tracks: Vec<TrackInfo>,
}

pub(crate) fn artist_refs(artists: &ArtistsWithRole) -> Vec<ArtistRef> {
    artists
        .iter()
        .filter_map(|artist| {
            Some(ArtistRef {
                uri: artist.id.to_uri().ok()?,
                name: artist.name.clone(),
            })
        })
        .collect()
}

pub(crate) fn year(date: &Date) -> i32 {
    date.as_utc().year()
}

pub(crate) fn largest_image(images: &Images) -> Option<String> {
    images
        .iter()
        .max_by_key(|image| image.width)
        .and_then(|image| image.id.to_base16().ok())
}

impl AlbumRef {
    pub(crate) fn from_album(album: &Album) -> Option<Self> {
        let covers = if album.covers.is_empty() {
            &album.cover_group
        } else {
            &album.covers
        };

        Some(Self {
            uri: album.id.to_uri().ok()?,
            name: album.name.clone(),
            year: year(&album.date),
            cover_id: largest_image(covers),
        })
    }
}

impl ArtistInfo {
    pub(crate) fn from_artist(
        artist: &Artist,
        top_tracks: Vec<TrackInfo>,
        albums: Vec<AlbumRef>,
        singles: Vec<AlbumRef>,
        compilations: Vec<AlbumRef>,
    ) -> Option<Self> {
        let portraits = if artist.portraits.is_empty() {
            &artist.portrait_group
        } else {
            &artist.portraits
        };

        Some(Self {
            uri: artist.id.to_uri().ok()?,
            name: artist.name.clone(),
            portrait_id: largest_image(portraits),
            biography: artist
                .biographies
                .first()
                .map(|biography| biography.text.clone()),
            top_tracks,
            albums,
            singles,
            compilations,
            related: artist
                .related
                .iter()
                .filter_map(|related| {
                    Some(ArtistRef {
                        uri: related.id.to_uri().ok()?,
                        name: related.name.clone(),
                    })
                })
                .collect(),
        })
    }
}

impl TrackInfo {
    pub(crate) fn from_track(track: &Track) -> Option<Self> {
        let covers = if track.album.covers.is_empty() {
            &track.album.cover_group
        } else {
            &track.album.covers
        };

        Some(Self {
            uri: track.id.to_uri().ok()?,
            name: track.name.clone(),
            artists: artist_refs(&track.artists_with_role),
            cover_id: largest_image(covers),
            number: track.number.max(0) as u32,
            disc: track.disc_number.max(0) as u32,
            duration_ms: track.duration.max(0) as u32,
            is_explicit: track.is_explicit,
        })
    }
}

impl AlbumInfo {
    pub(crate) fn from_album(album: &Album, tracks: Vec<TrackInfo>) -> Option<Self> {
        let covers = if album.covers.is_empty() {
            &album.cover_group
        } else {
            &album.covers
        };
        let cover_id = largest_image(covers);

        Some(Self {
            uri: album.id.to_uri().ok()?,
            name: album.name.clone(),
            artists: album
                .artists
                .iter()
                .filter_map(|artist| {
                    Some(ArtistRef {
                        uri: artist.id.to_uri().ok()?,
                        name: artist.name.clone(),
                    })
                })
                .collect(),
            year: year(&album.date),
            label: album.label.clone(),
            copyrights: album.copyrights.iter().map(|c| c.text.clone()).collect(),
            cover_id,
            tracks,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlaylistRef {
    pub uri: String,
    pub name: String,
    pub owner: String,
    pub length: usize,
    pub picture: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PlaylistInfo {
    pub uri: String,
    pub name: String,
    pub description: String,
    pub owner: String,
    pub updated_at_ms: i64,
    pub tracks: Vec<TrackInfo>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    pub name: String,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchType {
    Album,
    Artist,
    Playlist,
    Track,
    Show,
    Episode,
    Audiobook,
}

impl SearchType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Album => "album",
            Self::Artist => "artist",
            Self::Playlist => "playlist",
            Self::Track => "track",
            Self::Show => "show",
            Self::Episode => "episode",
            Self::Audiobook => "audiobook",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub types: Vec<SearchType>,
    pub market: Option<String>,
    pub limit: u8,
    pub offset: u32,
    pub include_external_audio: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            types: vec![
                SearchType::Album,
                SearchType::Artist,
                SearchType::Playlist,
                SearchType::Track,
                SearchType::Show,
                SearchType::Episode,
                SearchType::Audiobook,
            ],
            market: None,
            limit: 5,
            offset: 0,
            include_external_audio: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SearchResults {
    pub albums: Option<SearchPage<SearchAlbum>>,
    pub artists: Option<SearchPage<SearchArtist>>,
    pub playlists: Option<SearchPage<SearchPlaylist>>,
    pub tracks: Option<SearchPage<SearchTrack>>,
    pub shows: Option<SearchPage<SearchShow>>,
    pub episodes: Option<SearchPage<SearchEpisode>>,
    pub audiobooks: Option<SearchPage<SearchAudiobook>>,
}

#[derive(Debug, Clone)]
pub struct SearchPage<T> {
    pub href: String,
    pub items: Vec<T>,
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
    pub next: Option<String>,
    pub previous: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchAlbum {
    pub uri: String,
    pub name: String,
    pub artists: Vec<ArtistRef>,
    pub cover: Option<String>,
    pub release_date: String,
    pub total_tracks: u32,
    pub album_type: String,
}

#[derive(Debug, Clone)]
pub struct SearchArtist {
    pub uri: String,
    pub name: String,
    pub portrait: Option<String>,
    pub followers: u64,
    pub popularity: u32,
    pub genres: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SearchPlaylist {
    pub uri: String,
    pub name: String,
    pub description: String,
    pub owner: SearchOwner,
    pub picture: Option<String>,
    pub total_tracks: u32,
    pub collaborative: bool,
    pub public: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct SearchOwner {
    pub uri: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SearchTrack {
    pub uri: String,
    pub name: String,
    pub artists: Vec<ArtistRef>,
    pub album: SearchAlbum,
    pub duration_ms: u32,
    pub is_explicit: bool,
    pub is_playable: Option<bool>,
    pub popularity: u32,
    pub disc: u32,
    pub number: u32,
}

#[derive(Debug, Clone)]
pub struct SearchShow {
    pub uri: String,
    pub name: String,
    pub publisher: String,
    pub description: String,
    pub picture: Option<String>,
    pub total_episodes: u32,
    pub is_explicit: bool,
    pub media_type: String,
    pub languages: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SearchEpisode {
    pub uri: String,
    pub name: String,
    pub description: String,
    pub picture: Option<String>,
    pub duration_ms: u32,
    pub release_date: String,
    pub is_explicit: bool,
    pub is_playable: Option<bool>,
    pub languages: Vec<String>,
    pub audio_preview_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchAudiobook {
    pub uri: String,
    pub name: String,
    pub authors: Vec<String>,
    pub narrators: Vec<String>,
    pub description: String,
    pub edition: String,
    pub picture: Option<String>,
    pub total_chapters: u32,
    pub is_explicit: bool,
    pub media_type: String,
    pub languages: Vec<String>,
    pub publisher: String,
}
