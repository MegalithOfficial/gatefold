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
