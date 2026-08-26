use anyhow::Result;
use gatefold_core::{metadata, model::SearchOptions, session};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let session = session::connect().await?;
    let results = metadata::search(&session, "radiohead", &SearchOptions::default()).await?;

    println!(
        "albums: {}, artists: {}, playlists: {}, tracks: {}, shows: {}, episodes: {}, audiobooks: {}",
        results.albums.map_or(0, |page| page.items.len()),
        results.artists.map_or(0, |page| page.items.len()),
        results.playlists.map_or(0, |page| page.items.len()),
        results.tracks.map_or(0, |page| page.items.len()),
        results.shows.map_or(0, |page| page.items.len()),
        results.episodes.map_or(0, |page| page.items.len()),
        results.audiobooks.map_or(0, |page| page.items.len()),
    );

    Ok(())
}
