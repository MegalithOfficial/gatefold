use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use rand::seq::SliceRandom;

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
        artists: String,
        duration_ms: u32,
    },
    QueueChanged {
        index: usize,
        length: usize,
    },
    ShuffleChanged {
        shuffle: bool,
    },
    RepeatChanged {
        repeat: Repeat,
    },
    Volume {
        volume: u16,
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

#[derive(Default)]
struct Queue {
    uris: Vec<String>,
    order: Vec<usize>,
    position: usize,
    shuffled: bool,
}

impl Queue {
    fn current(&self) -> Option<String> {
        self.order
            .get(self.position)
            .and_then(|&index| self.uris.get(index))
            .cloned()
    }

    fn at(&self, position: usize) -> Option<String> {
        self.order
            .get(position)
            .and_then(|&index| self.uris.get(index))
            .cloned()
    }
}

pub struct Playback {
    player: Arc<Player>,
    mixer: SoftMixer,
    queue: Mutex<Queue>,
    repeat: Mutex<Repeat>,
    playing: AtomicBool,
    position_ms: AtomicU32,
    events: broadcast::Sender<Event>,
}

pub fn start(session: Session) -> Result<Arc<Playback>> {
    let backend =
        audio_backend::find(std::env::var("GATEFOLD_BACKEND").ok()).context("no audio backend")?;
    let mixer = SoftMixer::open(MixerConfig::default())?;

    let player = Player::new(
        PlayerConfig::default(),
        session,
        mixer.get_soft_volume(),
        move || backend(None, AudioFormat::default()),
    );

    let (events, _) = broadcast::channel(64);
    let playback = Arc::new(Playback {
        player,
        mixer,
        queue: Mutex::new(Queue::default()),
        repeat: Mutex::new(Repeat::Off),
        playing: AtomicBool::new(false),
        position_ms: AtomicU32::new(0),
        events,
    });

    let pump = playback.clone();
    let mut channel = pump.player.get_player_event_channel();
    tokio::spawn(async move {
        while let Some(event) = channel.recv().await {
            pump.handle(event);
        }
    });

    Ok(playback)
}

impl Playback {
    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub fn play_queue(&self, uris: Vec<String>, index: usize) {
        let length = uris.len();
        let position = index.min(length.saturating_sub(1));
        {
            let mut queue = self.queue.lock().unwrap();
            queue.order = (0..length).collect();
            queue.uris = uris;
            queue.position = position;
            queue.shuffled = false;
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

    pub fn set_repeat(&self, repeat: Repeat) {
        *self.repeat.lock().unwrap() = repeat;
        self.emit(Event::RepeatChanged { repeat });
    }

    pub fn play(&self) {
        self.player.play();
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn toggle(&self) {
        if self.playing.load(Ordering::Relaxed) {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&self, position_ms: u32) {
        self.player.seek(position_ms);
    }

    pub fn next(&self) {
        if !self.step(1) {
            self.player.stop();
        }
    }

    pub fn previous(&self) {
        if self.position_ms.load(Ordering::Relaxed) > RESTART_THRESHOLD_MS {
            self.player.seek(0);
        } else if !self.step(-1) {
            self.player.seek(0);
        }
    }

    pub fn volume(&self) -> u16 {
        self.mixer.volume()
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
            Repeat::Context => {
                if !self.step(1) {
                    let length = {
                        let mut queue = self.queue.lock().unwrap();
                        queue.position = 0;
                        queue.order.len()
                    };
                    self.emit(Event::QueueChanged { index: 0, length });
                    self.load_current();
                }
            }
            Repeat::Off => {
                if !self.step(1) {
                    self.player.stop();
                }
            }
        }
    }

    fn load_current(&self) {
        let uri = self.queue.lock().unwrap().current();

        let Some(uri) = uri else {
            return;
        };

        match SpotifyUri::from_uri(&uri) {
            Ok(id) => self.player.load(id, true, 0),
            Err(error) => tracing::error!("bad queue uri {uri}: {error}"),
        }
    }

    fn upcoming(&self) -> Option<String> {
        let queue = self.queue.lock().unwrap();
        queue
            .at(queue.position + 1)
            .or_else(|| match *self.repeat.lock().unwrap() {
                Repeat::Context => queue.at(0),
                _ => None,
            })
    }

    fn handle(&self, event: PlayerEvent) {
        match event {
            PlayerEvent::Loading { track_id, .. } => self.emit(Event::Loading {
                uri: uri_string(&track_id),
            }),
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
                let artists = match &audio_item.unique_fields {
                    UniqueFields::Track { artists, .. } => artists
                        .iter()
                        .map(|artist| artist.name.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                    UniqueFields::Local { artists, .. } => artists.clone().unwrap_or_default(),
                    UniqueFields::Episode { show_name, .. } => show_name.clone(),
                };
                self.emit(Event::TrackChanged {
                    uri: uri_string(&audio_item.track_id),
                    name: audio_item.name.clone(),
                    artists,
                    duration_ms: audio_item.duration_ms,
                });
            }
            PlayerEvent::EndOfTrack { .. } => {
                self.position_ms.store(0, Ordering::Relaxed);
                self.advance();
            }
            PlayerEvent::Unavailable { track_id, .. } => {
                tracing::warn!("track unavailable, skipping: {}", uri_string(&track_id));
                if !self.step(1) {
                    self.player.stop();
                }
            }
            PlayerEvent::TimeToPreloadNextTrack { .. } => {
                if let Some(uri) = self.upcoming() {
                    if let Ok(id) = SpotifyUri::from_uri(&uri) {
                        self.player.preload(id);
                    }
                }
            }
            PlayerEvent::VolumeChanged { volume } => self.emit(Event::Volume { volume }),
            PlayerEvent::Stopped { .. } => {
                self.playing.store(false, Ordering::Relaxed);
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
