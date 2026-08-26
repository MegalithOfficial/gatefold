use anyhow::{Context, Result};
use gatefold_core::{metadata, player, session};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("warn,librespot_playback=debug,librespot_audio=debug,gatefold_core=info")
        .with_writer(std::io::stderr)
        .init();

    let session = session::connect().await?;
    let playlists = metadata::playlists(&session).await?;
    let first = playlists.first().context("no playlists")?;
    eprintln!("queueing: {}", first.name);

    let uris = metadata::playlist_uris(&session, &first.uri).await?;
    let playback = player::start(session)?;
    let mut events = playback.events();
    playback.play_queue(uris, 0);

    let wait = tokio::time::timeout(std::time::Duration::from_secs(40), async {
        while let Ok(event) = events.recv().await {
            eprintln!("event: {event:?}");
            if matches!(event, player::Event::Playing { .. }) {
                break;
            }
        }
    });
    let _ = wait.await;

    Ok(())
}
