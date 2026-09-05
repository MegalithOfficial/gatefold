use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, RwLock, Weak,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

use rand::seq::SliceRandom;

use crate::{
    model::ArtistRef,
    session,
    sink::{Rodio, SinkHandle},
};

use anyhow::{Context, Result};
use librespot::{
    core::{Session, SpotifyUri},
    metadata::audio::UniqueFields,
    playback::{
        audio_backend,
        config::{AudioFormat, PlayerConfig},
        mixer::{Mixer, MixerConfig, softmixer::SoftMixer},
        player::{Player, PlayerEvent},
    },
};
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub enum Event {
    Loading {
        uri: String,
    },
    Playing {
        uri: String,
        position_ms: u32,
    },
    Paused {
        uri: String,
        position_ms: u32,
    },
    Position {
        uri: String,
        position_ms: u32,
    },
    TrackChanged {
        uri: String,
        name: String,
        artists: Vec<ArtistRef>,
        duration_ms: u32,
        cover_id: Option<String>,
    },
    QueueChanged {
        index: usize,
        length: usize,
    },
    UpNextChanged,
    ShuffleChanged {
        shuffle: bool,
    },
    RepeatChanged {
        repeat: Repeat,
    },
    Volume {
        volume: u16,
    },
    Connection {
        online: bool,
    },
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    Context,
    Track,
}

const RESTART_THRESHOLD_MS: u32 = 3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Queued(usize),
    Ahead(usize),
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub current: Option<String>,
    pub up_next: Vec<String>,
    pub source: String,
    pub ahead: Vec<String>,
    pub ahead_from: usize,
}

#[derive(Default)]
struct Queue {
    source: String,
    uris: Vec<String>,
    order: Vec<usize>,
    position: usize,
    shuffled: bool,
    up_next: VecDeque<String>,
    detour: Option<String>,
}

impl Queue {
    fn current(&self) -> Option<String> {
        self.detour.clone().or_else(|| self.at(self.position))
    }

    fn at(&self, position: usize) -> Option<String> {
        self.order
            .get(position)
            .and_then(|&index| self.uris.get(index))
            .cloned()
    }

    fn take_up_next(&mut self) -> bool {
        self.detour = self.up_next.pop_front();
        self.detour.is_some()
    }

    fn take_ahead(&mut self, offset: usize) -> Option<String> {
        let position = self.position + 1 + offset;
        if position >= self.order.len() {
            return None;
        }
        let index = self.order.remove(position);
        let uri = self.uris.remove(index);
        for slot in &mut self.order {
            if *slot > index {
                *slot -= 1;
            }
        }
        Some(uri)
    }

    fn put_ahead(&mut self, offset: usize, uri: String) {
        let position = (self.position + 1 + offset).min(self.order.len());
        self.uris.push(uri);
        self.order.insert(position, self.uris.len() - 1);
    }
}

pub struct Playback {
    weak: Weak<Playback>,
    session: RwLock<Session>,
    session_epoch: AtomicU64,
    player: RwLock<Arc<Player>>,
    player_epoch: AtomicU64,
    reconnecting: AtomicBool,
    resume: AtomicBool,
    mixer: SoftMixer,
    sink: SinkHandle,
    queue: Arc<Mutex<Queue>>,
    repeat: Mutex<Repeat>,
    playing: AtomicBool,
    stopped: AtomicBool,
    position_ms: AtomicU32,
    load_epoch: Arc<AtomicU64>,
    runtime: tokio::runtime::Handle,
    events: broadcast::Sender<Event>,
}

const LOAD_DEBOUNCE: Duration = Duration::from_millis(250);

pub fn start(session: Session) -> Result<Arc<Playback>> {
    let mixer = SoftMixer::open(MixerConfig::default())?;
    let handle = SinkHandle::default();
    let player = build_player(session.clone(), &mixer, &handle)?;

    let (events, _) = broadcast::channel(64);
    let playback = Arc::new_cyclic(|weak| Playback {
        weak: weak.clone(),
        session: RwLock::new(session),
        session_epoch: AtomicU64::new(0),
        player: RwLock::new(player.clone()),
        player_epoch: AtomicU64::new(0),
        reconnecting: AtomicBool::new(false),
        resume: AtomicBool::new(false),
        mixer,
        sink: handle,
        queue: Arc::new(Mutex::new(Queue::default())),
        repeat: Mutex::new(Repeat::Off),
        playing: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
        position_ms: AtomicU32::new(0),
        load_epoch: Arc::new(AtomicU64::new(0)),
        runtime: tokio::runtime::Handle::current(),
        events,
    });
    spawn_pump(playback.clone(), player);

    Ok(playback)
}

fn build_player(session: Session, mixer: &SoftMixer, sink: &SinkHandle) -> Result<Arc<Player>> {
    Ok(match std::env::var("GATEFOLD_BACKEND").ok() {
        Some(name) => {
            let backend = audio_backend::find(Some(name)).context("no such audio backend")?;
            Player::new(
                PlayerConfig::default(),
                session,
                mixer.get_soft_volume(),
                move || backend(None, AudioFormat::default()),
            )
        }
        None => {
            let sink = sink.clone();
            Player::new(
                PlayerConfig::default(),
                session,
                mixer.get_soft_volume(),
                move || Box::new(Rodio::open(&sink).expect("audio output")),
            )
        }
    })
}

fn spawn_pump(playback: Arc<Playback>, player: Arc<Player>) {
    let mut channel = player.get_player_event_channel();
    playback.runtime.clone().spawn(async move {
        while let Some(event) = channel.recv().await {
            playback.handle(event);
        }
    });
}

impl Playback {
    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub fn session(&self) -> Session {
        self.session.read().unwrap().clone()
    }

    fn player(&self) -> Arc<Player> {
        self.player.read().unwrap().clone()
    }

    fn reconnect(&self) {
        if self.reconnecting.swap(true, Ordering::SeqCst) {
            return;
        }
        tracing::warn!("session lost, reconnecting");
        self.emit(Event::Connection { online: false });
        let weak = self.weak.clone();
        self.runtime.spawn(async move {
            let mut delay = 1;
            loop {
                let Some(playback) = weak.upgrade() else {
                    return;
                };
                if !session::signed_in() {
                    playback.reconnecting.store(false, Ordering::SeqCst);
                    return;
                }
                match session::resume().await {
                    Ok(session) => {
                        *playback.session.write().unwrap() = session;
                        playback.session_epoch.fetch_add(1, Ordering::SeqCst);
                        playback.reconnecting.store(false, Ordering::SeqCst);
                        playback.emit(Event::Connection { online: true });
                        tracing::info!("session reconnected");
                        if playback.resume.swap(false, Ordering::SeqCst) {
                            playback.load_current();
                        }
                        return;
                    }
                    Err(error) => {
                        tracing::warn!("reconnect failed, retrying in {delay}s: {error:#}");
                    }
                }
                drop(playback);
                tokio::time::sleep(Duration::from_secs(delay)).await;
                delay = (delay * 2).min(30);
            }
        });
    }

    fn refresh_player(&self) {
        let generation = self.session_epoch.load(Ordering::SeqCst);
        if self.player_epoch.swap(generation, Ordering::SeqCst) == generation {
            return;
        }
        match build_player(self.session(), &self.mixer, &self.sink) {
            Ok(player) => {
                let old = {
                    let mut slot = self.player.write().unwrap();
                    std::mem::replace(&mut *slot, player.clone())
                };
                old.stop();
                if let Some(playback) = self.weak.upgrade() {
                    spawn_pump(playback, player);
                }
            }
            Err(error) => tracing::error!("could not rebuild the player: {error:#}"),
        }
    }

    pub fn play_queue(&self, source: &str, uris: Vec<String>, index: usize) {
        let length = uris.len();
        let position = index.min(length.saturating_sub(1));
        {
            let mut queue = self.queue.lock().unwrap();
            queue.source = source.to_owned();
            queue.order = (0..length).collect();
            queue.uris = uris;
            queue.position = position;
            queue.shuffled = false;
            queue.detour = None;
        }
        self.emit(Event::QueueChanged {
            index: position,
            length,
        });
        self.load_current();
    }

    pub fn queue(&self) -> (Vec<String>, usize) {
        let queue = self.queue.lock().unwrap();
        let ordered = queue
            .order
            .iter()
            .filter_map(|&index| queue.uris.get(index).cloned())
            .collect();

        (ordered, queue.position)
    }

    pub fn current(&self) -> Option<String> {
        self.queue.lock().unwrap().current()
    }

    pub fn snapshot(&self) -> Snapshot {
        let queue = self.queue.lock().unwrap();
        let ahead_from = if queue.order.is_empty() {
            0
        } else {
            queue.position + 1
        };
        Snapshot {
            current: queue.current(),
            up_next: queue.up_next.iter().cloned().collect(),
            source: queue.source.clone(),
            ahead: (ahead_from..queue.order.len())
                .filter_map(|position| queue.at(position))
                .collect(),
            ahead_from,
        }
    }

    pub fn play_next(&self, uris: Vec<String>) {
        {
            let mut queue = self.queue.lock().unwrap();
            for uri in uris.into_iter().rev() {
                queue.up_next.push_front(uri);
            }
        }
        self.emit(Event::UpNextChanged);
    }

    pub fn add_to_queue(&self, uris: Vec<String>) {
        self.queue.lock().unwrap().up_next.extend(uris);
        self.emit(Event::UpNextChanged);
    }

    pub fn remove_up_next(&self, index: usize) {
        if self.queue.lock().unwrap().up_next.remove(index).is_some() {
            self.emit(Event::UpNextChanged);
        }
    }

    pub fn move_track(&self, from: Slot, to: Slot) {
        let (index, length) = {
            let mut queue = self.queue.lock().unwrap();
            let uri = match from {
                Slot::Queued(index) => queue.up_next.remove(index),
                Slot::Ahead(offset) => queue.take_ahead(offset),
            };
            let Some(uri) = uri else {
                return;
            };
            match to {
                Slot::Queued(index) => {
                    let index = index.min(queue.up_next.len());
                    queue.up_next.insert(index, uri);
                }
                Slot::Ahead(offset) => queue.put_ahead(offset, uri),
            }
            (queue.position, queue.order.len())
        };
        self.emit(Event::UpNextChanged);
        if matches!(from, Slot::Ahead(_)) || matches!(to, Slot::Ahead(_)) {
            self.emit(Event::QueueChanged { index, length });
        }
    }

    pub fn clear_up_next(&self) {
        self.queue.lock().unwrap().up_next.clear();
        self.emit(Event::UpNextChanged);
    }

    pub fn play_up_next(&self, index: usize) {
        {
            let mut queue = self.queue.lock().unwrap();
            if index >= queue.up_next.len() {
                return;
            }
            queue.up_next.drain(..index);
            queue.take_up_next();
        }
        self.emit(Event::UpNextChanged);
        self.load_current();
    }

    pub fn jump(&self, position: usize) {
        let length = {
            let mut queue = self.queue.lock().unwrap();
            if position >= queue.order.len() {
                return;
            }
            queue.position = position;
            queue.detour = None;
            queue.order.len()
        };
        self.emit(Event::QueueChanged {
            index: position,
            length,
        });
        self.load_current();
    }

    fn take_up_next(&self) -> bool {
        let taken = self.queue.lock().unwrap().take_up_next();
        if taken {
            self.emit(Event::UpNextChanged);
            self.load_current();
        }
        taken
    }

    pub fn set_shuffle(&self, shuffle: bool) {
        {
            let mut queue = self.queue.lock().unwrap();
            if queue.shuffled == shuffle || queue.uris.is_empty() {
                return;
            }

            let current = queue.order.get(queue.position).copied().unwrap_or(0);
            if shuffle {
                let mut rest: Vec<usize> =
                    (0..queue.uris.len()).filter(|&i| i != current).collect();
                rest.shuffle(&mut rand::rng());
                let mut order = vec![current];
                order.extend(rest);
                queue.order = order;
                queue.position = 0;
            } else {
                queue.order = (0..queue.uris.len()).collect();
                queue.position = current;
            }
            queue.shuffled = shuffle;
        }
        self.emit(Event::ShuffleChanged { shuffle });
    }

    pub fn shuffle(&self) -> bool {
        self.queue.lock().unwrap().shuffled
    }

    pub fn repeat(&self) -> Repeat {
        *self.repeat.lock().unwrap()
    }

    pub fn set_repeat(&self, repeat: Repeat) {
        *self.repeat.lock().unwrap() = repeat;
        self.emit(Event::RepeatChanged { repeat });
    }

    pub fn play(&self) {
        let idle = self.queue.lock().unwrap().current().is_none();
        if idle {
            self.take_up_next();
        } else if self.stopped.load(Ordering::Relaxed) {
            self.load_current();
        } else {
            self.player().play();
        }
    }

    pub fn pause(&self) {
        self.player().pause();
    }

    pub fn stop(&self) {
        self.player().stop();
    }

    pub fn toggle(&self) {
        if self.playing.load(Ordering::Relaxed) {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&self, position_ms: u32) {
        self.sink.flush();
        self.player().seek(position_ms);
    }

    pub fn next(&self) {
        self.demote_repeat();
        if !self.take_up_next() && !self.step(1) {
            self.player().stop();
        }
    }

    pub fn previous(&self) {
        if self.position_ms.load(Ordering::Relaxed) > RESTART_THRESHOLD_MS {
            self.player().seek(0);
        } else {
            self.demote_repeat();
            let detoured = self.queue.lock().unwrap().detour.take().is_some();
            if detoured {
                self.load_current();
            } else if !self.step(-1) {
                self.player().seek(0);
            }
        }
    }

    fn demote_repeat(&self) {
        let mut repeat = self.repeat.lock().unwrap();
        if *repeat == Repeat::Track {
            *repeat = Repeat::Context;
            drop(repeat);
            self.emit(Event::RepeatChanged {
                repeat: Repeat::Context,
            });
        }
    }

    pub fn volume(&self) -> u16 {
        self.mixer.volume()
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    pub fn set_volume(&self, volume: u16) {
        self.mixer.set_volume(volume);
        self.emit(Event::Volume { volume });
    }

    fn step(&self, delta: isize) -> bool {
        let next = {
            let mut queue = self.queue.lock().unwrap();
            let position = queue.position as isize + delta;
            if position < 0 || position as usize >= queue.order.len() {
                None
            } else {
                queue.position = position as usize;
                queue.detour = None;
                Some((position as usize, queue.order.len()))
            }
        };

        match next {
            Some((index, length)) => {
                self.emit(Event::QueueChanged { index, length });
                self.load_current();
                true
            }
            None => false,
        }
    }

    fn advance(&self) {
        let repeat = *self.repeat.lock().unwrap();
        match repeat {
            Repeat::Track => self.load_current(),
            _ if self.take_up_next() => {}
            Repeat::Context => {
                if !self.step(1) {
                    let length = {
                        let mut queue = self.queue.lock().unwrap();
                        queue.position = 0;
                        queue.detour = None;
                        queue.order.len()
                    };
                    self.emit(Event::QueueChanged { index: 0, length });
                    self.load_current();
                }
            }
            Repeat::Off => {
                if !self.step(1) {
                    self.player().stop();
                }
            }
        }
    }

    fn load_current(&self) {
        let epoch = self.load_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        let load_epoch = self.load_epoch.clone();
        let weak = self.weak.clone();

        self.runtime.spawn(async move {
            tokio::time::sleep(LOAD_DEBOUNCE).await;
            if load_epoch.load(Ordering::SeqCst) != epoch {
                return;
            }
            let Some(playback) = weak.upgrade() else {
                return;
            };
            let uri = playback.queue.lock().unwrap().current();
            let Some(uri) = uri else {
                return;
            };
            if playback.session().is_invalid() {
                playback.resume.store(true, Ordering::SeqCst);
                playback.reconnect();
                return;
            }
            playback.refresh_player();

            match SpotifyUri::from_uri(&uri) {
                Ok(id) => {
                    playback.sink.flush();
                    playback.player().load(id, true, 0);
                }
                Err(error) => tracing::error!("bad queue uri {uri}: {error}"),
            }
        });
    }

    fn upcoming(&self) -> Option<String> {
        let queue = self.queue.lock().unwrap();
        queue
            .up_next
            .front()
            .cloned()
            .or_else(|| queue.at(queue.position + 1))
            .or_else(|| match *self.repeat.lock().unwrap() {
                Repeat::Context => queue.at(0),
                _ => None,
            })
    }

    fn handle(&self, event: PlayerEvent) {
        match event {
            PlayerEvent::Loading { track_id, .. } => {
                self.stopped.store(false, Ordering::Relaxed);
                self.emit(Event::Loading {
                    uri: uri_string(&track_id),
                });
            }
            PlayerEvent::Playing {
                track_id,
                position_ms,
                ..
            } => {
                self.playing.store(true, Ordering::Relaxed);
                self.position_ms.store(position_ms, Ordering::Relaxed);
                self.emit(Event::Playing {
                    uri: uri_string(&track_id),
                    position_ms,
                });
            }
            PlayerEvent::Paused {
                track_id,
                position_ms,
                ..
            } => {
                self.playing.store(false, Ordering::Relaxed);
                self.position_ms.store(position_ms, Ordering::Relaxed);
                self.emit(Event::Paused {
                    uri: uri_string(&track_id),
                    position_ms,
                });
            }
            PlayerEvent::PositionChanged {
                track_id,
                position_ms,
                ..
            }
            | PlayerEvent::PositionCorrection {
                track_id,
                position_ms,
                ..
            }
            | PlayerEvent::Seeked {
                track_id,
                position_ms,
                ..
            } => {
                self.position_ms.store(position_ms, Ordering::Relaxed);
                self.emit(Event::Position {
                    uri: uri_string(&track_id),
                    position_ms,
                });
            }
            PlayerEvent::TrackChanged { audio_item } => {
                let unlinked = |name: &String| ArtistRef {
                    uri: String::new(),
                    name: name.clone(),
                };
                let artists = match &audio_item.unique_fields {
                    UniqueFields::Track { artists, .. } => artists
                        .iter()
                        .map(|artist| ArtistRef {
                            uri: uri_string(&artist.id),
                            name: artist.name.clone(),
                        })
                        .collect(),
                    UniqueFields::Local { artists, .. } => artists.iter().map(unlinked).collect(),
                    UniqueFields::Episode { show_name, .. } => vec![unlinked(show_name)],
                };
                let cover_id = audio_item
                    .covers
                    .iter()
                    .max_by_key(|cover| cover.width)
                    .and_then(|cover| cover.url.rsplit('/').next())
                    .filter(|id| id.len() == 40 && id.chars().all(|c| c.is_ascii_hexdigit()))
                    .map(str::to_owned);
                self.emit(Event::TrackChanged {
                    uri: uri_string(&audio_item.track_id),
                    name: audio_item.name.clone(),
                    artists,
                    duration_ms: audio_item.duration_ms,
                    cover_id,
                });
            }
            PlayerEvent::EndOfTrack { .. } => {
                self.position_ms.store(0, Ordering::Relaxed);
                self.advance();
            }
            PlayerEvent::Unavailable { track_id, .. } => {
                if self.session().is_invalid() {
                    self.resume.store(true, Ordering::SeqCst);
                    self.reconnect();
                    return;
                }
                tracing::warn!("track unavailable, skipping: {}", uri_string(&track_id));
                if !self.take_up_next() && !self.step(1) {
                    self.player().stop();
                }
            }
            PlayerEvent::TimeToPreloadNextTrack { .. } => {
                if self.session().is_invalid() {
                    return;
                }
                if let Some(uri) = self.upcoming()
                    && let Ok(id) = SpotifyUri::from_uri(&uri)
                {
                    self.player().preload(id);
                }
            }
            PlayerEvent::VolumeChanged { volume } => self.emit(Event::Volume { volume }),
            PlayerEvent::Stopped { .. } => {
                self.playing.store(false, Ordering::Relaxed);
                self.stopped.store(true, Ordering::Relaxed);
                self.emit(Event::Stopped);
            }
            _ => {}
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }
}

fn uri_string(uri: &SpotifyUri) -> String {
    uri.to_uri().unwrap_or_default()
}
