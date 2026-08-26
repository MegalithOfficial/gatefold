use librespot::core::date::Date;
use librespot::metadata::artist::ArtistsWithRole;
use librespot::metadata::{Album, Track};

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
    pub number: u32,
    pub disc: u32,
    pub duration_ms: u32,
    pub is_explicit: bool,
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

impl TrackInfo {
    pub(crate) fn from_track(track: &Track) -> Option<Self> {
        Some(Self {
            uri: track.id.to_uri().ok()?,
            name: track.name.clone(),
            artists: artist_refs(&track.artists_with_role),
            number: track.number.max(0) as u32,
            disc: track.disc_number.max(0) as u32,
            duration_ms: track.duration.max(0) as u32,
            is_explicit: track.is_explicit,
        })
    }
}

impl AlbumInfo {
    pub(crate) fn from_album(album: &Album, tracks: Vec<TrackInfo>) -> Option<Self> {
        let cover_id = album
            .covers
            .iter()
            .chain(album.cover_group.iter())
            .max_by_key(|image| image.width)
            .and_then(|image| image.id.to_base16().ok());

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

#[derive(Debug, Clone)]
pub struct PlaylistRef {
    pub uri: String,
    pub name: String,
    pub owner: String,
    pub length: usize,
}

#[derive(Debug, Clone)]
pub struct PlaylistInfo {
    pub uri: String,
    pub name: String,
    pub description: String,
    pub owner: String,
    pub tracks: Vec<TrackInfo>,
}
