use std::{
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
    time::Instant,
};

use gatefold_core::{
    images,
    player::{Event, Playback, Repeat},
};
use mpris_server::{
    LoopStatus, Metadata, PlaybackRate, PlaybackStatus, PlayerInterface, Property, RootInterface,
    Server, Signal, Time, TrackId, Volume,
    zbus::{self, fdo},
};
use relm4::{
    Sender,
    gtk::{gio, prelude::FileExt},
};
use tokio::sync::{broadcast::error::RecvError, mpsc};

use crate::{APP_ID, app::AppAction};

const SEEK_TOLERANCE_MS: u32 = 1000;

pub struct Mpris {
    covers: mpsc::UnboundedSender<PathBuf>,
}

impl Mpris {
    pub fn start(playback: &Arc<Playback>, app: Sender<AppAction>) -> Mpris {
        let (covers, mut incoming) = mpsc::unbounded_channel();
        let mut events = playback.events();
        let bridge = Bridge {
            playback: Arc::downgrade(playback),
            app,
            state: Mutex::default(),
        };
        relm4::spawn(async move {
            let server = match Server::new("gatefold", bridge).await {
                Ok(server) => server,
                Err(error) => {
                    tracing::warn!("mpris unavailable: {error}");
                    return;
                }
            };
            loop {
                let (changed, seeked) = tokio::select! {
                    event = events.recv() => match event {
                        Ok(event) => server.imp().apply(event),
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    },
                    cover = incoming.recv() => match cover {
                        Some(path) => server.imp().cover(path),
                        None => break,
                    },
                };
                if let Err(error) = server.properties_changed(changed).await {
                    tracing::debug!("mpris properties: {error}");
                }
                if let Some(position) = seeked {
                    let signal = Signal::Seeked {
                        position: Time::from_millis(position as i64),
                    };
                    if let Err(error) = server.emit(signal).await {
                        tracing::debug!("mpris seeked: {error}");
                    }
                }
            }
        });
        Mpris { covers }
    }

    pub fn cover(&self, path: PathBuf) {
        let _ = self.covers.send(path);
    }
}

#[derive(Default)]
struct State {
    uri: Option<String>,
    name: String,
    artists: Vec<String>,
    duration_ms: u32,
    cover: Option<PathBuf>,
    position_ms: u32,
    since: Option<Instant>,
}

impl State {
    fn position_ms(&self) -> u32 {
        match self.since {
            Some(since) => self.position_ms + since.elapsed().as_millis() as u32,
            None => self.position_ms,
        }
        .min(self.duration_ms.max(self.position_ms))
    }

    fn set_position(&mut self, position_ms: u32, playing: bool) {
        self.position_ms = position_ms;
        self.since = playing.then(Instant::now);
    }

    fn track_id(&self) -> TrackId {
        self.uri
            .as_deref()
            .and_then(|uri| uri.rsplit(':').next())
            .and_then(|id| {
                TrackId::try_from(format!("/io/github/megalithofficial/gatefold/track/{id}")).ok()
            })
            .unwrap_or(TrackId::NO_TRACK)
    }

    fn metadata(&self) -> Metadata {
        let mut metadata = Metadata::builder().trackid(self.track_id());
        if let Some(uri) = &self.uri {
            metadata = metadata
                .title(self.name.clone())
                .artist(self.artists.clone())
                .length(Time::from_millis(self.duration_ms as i64))
                .url(uri.clone());
        }
        if let Some(cover) = &self.cover {
            metadata = metadata.art_url(gio::File::for_path(cover).uri());
        }
        metadata.build()
    }
}

struct Bridge {
    playback: Weak<Playback>,
    app: Sender<AppAction>,
    state: Mutex<State>,
}

type Changes = (Vec<Property>, Option<u32>);

impl Bridge {
    fn apply(&self, event: Event) -> Changes {
        let mut state = self.state.lock().unwrap();
        match event {
            Event::Playing { position_ms, .. } => {
                state.set_position(position_ms, true);
                (
                    vec![Property::PlaybackStatus(PlaybackStatus::Playing)],
                    None,
                )
            }
            Event::Paused { position_ms, .. } => {
                state.set_position(position_ms, false);
                (vec![Property::PlaybackStatus(PlaybackStatus::Paused)], None)
            }
            Event::Position { position_ms, .. } => {
                let drift = state.position_ms().abs_diff(position_ms);
                let playing = state.since.is_some();
                state.set_position(position_ms, playing);
                (
                    Vec::new(),
                    (drift > SEEK_TOLERANCE_MS).then_some(position_ms),
                )
            }
            Event::TrackChanged {
                uri,
                name,
                artists,
                duration_ms,
                cover_id,
            } => {
                state.uri = Some(uri);
                state.name = name;
                state.artists = artists.into_iter().map(|artist| artist.name).collect();
                state.duration_ms = duration_ms;
                state.cover = cover_id.as_deref().and_then(images::cached);
                state.set_position(0, false);
                (vec![Property::Metadata(state.metadata())], None)
            }
            Event::Stopped => {
                state.set_position(0, false);
                (
                    vec![Property::PlaybackStatus(PlaybackStatus::Stopped)],
                    None,
                )
            }
            Event::ShuffleChanged { shuffle } => (vec![Property::Shuffle(shuffle)], None),
            Event::RepeatChanged { repeat } => {
                (vec![Property::LoopStatus(loop_status(repeat))], None)
            }
            Event::Volume { volume } => (
                vec![Property::Volume(volume as f64 / u16::MAX as f64)],
                None,
            ),
            Event::Loading { .. }
            | Event::QueueChanged { .. }
            | Event::UpNextChanged
            | Event::Connection { .. } => (Vec::new(), None),
        }
    }

    fn cover(&self, path: PathBuf) -> Changes {
        let mut state = self.state.lock().unwrap();
        state.cover = Some(path);
        (vec![Property::Metadata(state.metadata())], None)
    }

    fn playback(&self) -> fdo::Result<Arc<Playback>> {
        self.playback
            .upgrade()
            .ok_or_else(|| fdo::Error::Failed("signed out".into()))
    }
}

fn loop_status(repeat: Repeat) -> LoopStatus {
    match repeat {
        Repeat::Off => LoopStatus::None,
        Repeat::Context => LoopStatus::Playlist,
        Repeat::Track => LoopStatus::Track,
    }
}

impl RootInterface for Bridge {
    async fn raise(&self) -> fdo::Result<()> {
        self.app.emit(AppAction::Raise);
        Ok(())
    }

    async fn quit(&self) -> fdo::Result<()> {
        self.app.emit(AppAction::Quit);
        Ok(())
    }

    async fn can_quit(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn set_fullscreen(&self, _: bool) -> zbus::Result<()> {
        Ok(())
    }

    async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn can_raise(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn has_track_list(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn identity(&self) -> fdo::Result<String> {
        Ok("Gatefold".into())
    }

    async fn desktop_entry(&self) -> fdo::Result<String> {
        Ok(APP_ID.into())
    }

    async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
        Ok(vec!["spotify".into()])
    }

    async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
        Ok(Vec::new())
    }
}

impl PlayerInterface for Bridge {
    async fn next(&self) -> fdo::Result<()> {
        self.playback()?.next();
        Ok(())
    }

    async fn previous(&self) -> fdo::Result<()> {
        self.playback()?.previous();
        Ok(())
    }

    async fn pause(&self) -> fdo::Result<()> {
        self.playback()?.pause();
        Ok(())
    }

    async fn play_pause(&self) -> fdo::Result<()> {
        self.playback()?.toggle();
        Ok(())
    }

    async fn stop(&self) -> fdo::Result<()> {
        self.playback()?.stop();
        Ok(())
    }

    async fn play(&self) -> fdo::Result<()> {
        self.playback()?.play();
        Ok(())
    }

    async fn seek(&self, offset: Time) -> fdo::Result<()> {
        let playback = self.playback()?;
        let target = {
            let state = self.state.lock().unwrap();
            (state.position_ms() as i64 + offset.as_millis()).clamp(0, state.duration_ms as i64)
        };
        playback.seek(target as u32);
        Ok(())
    }

    async fn set_position(&self, track_id: TrackId, position: Time) -> fdo::Result<()> {
        let playback = self.playback()?;
        let target = {
            let state = self.state.lock().unwrap();
            if state.track_id() != track_id {
                return Ok(());
            }
            position.as_millis().clamp(0, state.duration_ms as i64)
        };
        playback.seek(target as u32);
        Ok(())
    }

    async fn open_uri(&self, uri: String) -> fdo::Result<()> {
        if !uri.starts_with("spotify:track:") {
            return Err(fdo::Error::NotSupported(uri));
        }
        self.playback()?.play_queue(&uri, vec![uri.clone()], 0);
        Ok(())
    }

    async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
        let playback = self.playback()?;
        Ok(if playback.is_playing() {
            PlaybackStatus::Playing
        } else if playback.is_stopped() || playback.current().is_none() {
            PlaybackStatus::Stopped
        } else {
            PlaybackStatus::Paused
        })
    }

    async fn loop_status(&self) -> fdo::Result<LoopStatus> {
        Ok(loop_status(self.playback()?.repeat()))
    }

    async fn set_loop_status(&self, loop_status: LoopStatus) -> zbus::Result<()> {
        self.playback()?.set_repeat(match loop_status {
            LoopStatus::None => Repeat::Off,
            LoopStatus::Playlist => Repeat::Context,
            LoopStatus::Track => Repeat::Track,
        });
        Ok(())
    }

    async fn rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn set_rate(&self, _: PlaybackRate) -> zbus::Result<()> {
        Ok(())
    }

    async fn shuffle(&self) -> fdo::Result<bool> {
        Ok(self.playback()?.shuffle())
    }

    async fn set_shuffle(&self, shuffle: bool) -> zbus::Result<()> {
        self.playback()?.set_shuffle(shuffle);
        Ok(())
    }

    async fn metadata(&self) -> fdo::Result<Metadata> {
        Ok(self.state.lock().unwrap().metadata())
    }

    async fn volume(&self) -> fdo::Result<Volume> {
        Ok(self.playback()?.volume() as f64 / u16::MAX as f64)
    }

    async fn set_volume(&self, volume: Volume) -> zbus::Result<()> {
        self.playback()?
            .set_volume((volume.clamp(0.0, 1.0) * u16::MAX as f64) as u16);
        Ok(())
    }

    async fn position(&self) -> fdo::Result<Time> {
        Ok(Time::from_millis(
            self.state.lock().unwrap().position_ms() as i64
        ))
    }

    async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn can_go_next(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn can_go_previous(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn can_play(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn can_pause(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn can_seek(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn can_control(&self) -> fdo::Result<bool> {
        Ok(true)
    }
}
