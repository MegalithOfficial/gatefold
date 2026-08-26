use anyhow::{Context, Result};
use gatefold_core::{images, metadata, session};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let session = session::connect().await?;
    println!("connected as {}", session.username());
    match session::profile(&session).await {
        Ok(profile) => println!(
            "profile: {} avatar: {}",
            profile.name,
            profile.avatar.is_some()
        ),
        Err(error) => println!("profile failed: {error:#}"),
    }

    let playlists = metadata::playlists(&session).await?;
    println!("\nplaylists: {}", playlists.len());
    for playlist in playlists.iter().take(5) {
        println!(
            "  {} ({} tracks, art: {})",
            playlist.name,
            playlist.length,
            playlist.picture.is_some()
        );
    }

    let first = playlists.first().context("no playlists")?;
    let playlist = metadata::playlist(&session, &first.uri).await?;
    println!(
        "\n{}: {} resolved tracks",
        playlist.name,
        playlist.tracks.len()
    );
    for track in playlist.tracks.iter().take(3) {
        let artists: Vec<&str> = track.artists.iter().map(|a| a.name.as_str()).collect();
        println!(
            "  {} - {} ({}ms)",
            artists.join(", "),
            track.name,
            track.duration_ms
        );
    }

    let track = playlist.tracks.first().context("empty playlist")?;
    let raw = metadata::track(&session, &track.uri).await?;
    let album_uri = raw.album.id.to_uri()?;
    let album = metadata::album(&session, &album_uri).await?;
    println!(
        "\nalbum: {} ({}) by {} - {} tracks, label {}, cover {:?}",
        album.name,
        album.year,
        album
            .artists
            .first()
            .map(|a| a.name.as_str())
            .unwrap_or("?"),
        album.tracks.len(),
        album.label,
        album.cover_id.as_deref().map(|id| &id[..8.min(id.len())]),
    );

    if let Some(cover) = &album.cover_id {
        let path = images::fetch(&session, cover).await?;
        println!(
            "cover cached at {} ({} bytes)",
            path.display(),
            std::fs::metadata(&path)?.len()
        );
    }

    let artist_uri = &album.artists.first().context("album has no artist")?.uri;
    let artist = metadata::artist(&session, artist_uri).await?;
    println!(
        "\nartist: {} - {} top tracks, {} albums, {} singles, {} compilations, {} related, bio {}",
        artist.name,
        artist.top_tracks.len(),
        artist.albums.len(),
        artist.singles.len(),
        artist.compilations.len(),
        artist.related.len(),
        artist.biography.as_deref().map(|b| b.len()).unwrap_or(0),
    );

    Ok(())
}
