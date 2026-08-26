use std::{path::PathBuf, sync::Arc, time::Duration};

use gatefold_core::player;
use relm4::{Component, ComponentParts, ComponentSender, gtk, gtk::prelude::*};

use crate::app::{Services, Track};

pub const CSS: &str = include_str!("style.css");

pub struct Deck {
    services: Option<Arc<Services>>,
    cover: Option<PathBuf>,
    title: String,
    artist: String,
    playing: bool,
    position_ms: u32,
    duration_ms: u32,
    volume: f64,
}

pub enum DeckAction {
    SetTrack(Arc<Services>, Track),
    Toggle,
    Previous,
    Next,
    Seek(f64),
    Volume(f64),
}

impl std::fmt::Debug for DeckAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeckAction::SetTrack(_, track) => write!(f, "SetTrack({})", track.title),
            action => write!(f, "{action:?}"),
        }
    }
}

#[derive(Debug)]
pub enum DeckUpdate {
    Playback(player::Event),
    Tick,
}

#[relm4::component(pub)]
impl Component for Deck {
    type Init = ();
    type Input = DeckAction;
    type Output = ();
    type CommandOutput = DeckUpdate;

    view! {
        gtk::CenterBox {
            add_css_class: "deck",

            #[wrap(Some)]
            set_start_widget = &gtk::Box {
                set_spacing: 12,
                set_valign: gtk::Align::Center,

                gtk::Image {
                    #[watch]
                    set_from_file: model.cover.as_ref(),
                    set_pixel_size: 52,
                    add_css_class: "thumb",
                },

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_valign: gtk::Align::Center,

                    gtk::Label {
                        #[watch]
                        set_label: &model.title,
                        set_xalign: 0.0,
                        set_width_chars: 8,
                        set_max_width_chars: 16,
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        add_css_class: "now-title",
                    },

                    gtk::Label {
                        #[watch]
                        set_label: &model.artist,
                        set_xalign: 0.0,
                        set_width_chars: 8,
                        set_max_width_chars: 16,
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        add_css_class: "now-artist",
                    },
                },
            },

            #[wrap(Some)]
            set_center_widget = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 2,
                set_valign: gtk::Align::Center,

                gtk::Box {
                    set_spacing: 6,
                    set_halign: gtk::Align::Center,

                    gtk::Button {
                        set_icon_name: "media-skip-backward-symbolic",
                        add_css_class: "icon",
                        connect_clicked => DeckAction::Previous,
                    },

                    gtk::Button {
                        #[watch]
                        set_icon_name: if model.playing {
                            "media-playback-pause-symbolic"
                        } else {
                            "media-playback-start-symbolic"
                        },
                        #[watch]
                        set_sensitive: model.services.is_some(),
                        add_css_class: "pause",
                        connect_clicked => DeckAction::Toggle,
                    },

                    gtk::Button {
                        set_icon_name: "media-skip-forward-symbolic",
                        add_css_class: "icon",
                        connect_clicked => DeckAction::Next,
                    },
                },

                gtk::Box {
                    set_spacing: 10,

                    gtk::Label {
                        #[watch]
                        set_label: &clock(model.position_ms),
                        set_xalign: 1.0,
                        add_css_class: "now-time",
                    },

                    gtk::Scale {
                        set_hexpand: true,
                        set_size_request: (160, -1),
                        set_valign: gtk::Align::Center,
                        add_css_class: "seek",
                        #[watch]
                        set_range: (0.0, model.duration_ms.max(1) as f64),
                        #[watch]
                        #[block_signal(seek_handler)]
                        set_value: model.position_ms as f64,
                        connect_change_value[sender] => move |_, _, value| {
                            sender.input(DeckAction::Seek(value));
                            gtk::glib::Propagation::Proceed
                        } @seek_handler,
                    },

                    gtk::Label {
                        #[watch]
                        set_label: &clock(model.duration_ms),
                        set_xalign: 0.0,
                        add_css_class: "now-time",
                    },
                },
            },

            #[wrap(Some)]
            set_end_widget = &gtk::Box {
                add_css_class: "volume",
                set_spacing: 6,
                set_valign: gtk::Align::Center,

                gtk::Image {
                    set_icon_name: Some("audio-volume-medium-symbolic"),
                },

                gtk::Scale {
                    set_size_request: (90, -1),
                    set_valign: gtk::Align::Center,
                    set_range: (0.0, 100.0),
                    #[watch]
                    #[block_signal(volume_handler)]
                    set_value: model.volume,
                    connect_change_value[sender] => move |_, _, value| {
                        sender.input(DeckAction::Volume(value));
                        gtk::glib::Propagation::Proceed
                    } @volume_handler,
                },
            },
        }
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Deck {
            services: None,
            cover: None,
            title: String::new(),
            artist: String::new(),
            playing: false,
            position_ms: 0,
            duration_ms: 0,
            volume: 100.0,
        };
        let widgets = view_output!();
        let _ = sender;

        ComponentParts { model, widgets }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        if let DeckAction::SetTrack(services, track) = action {
            self.cover = Some(track.cover);
            self.title = track.title;
            self.artist = track.artist;
            self.duration_ms = track.duration_ms;
            self.volume = services.playback.volume() as f64 / u16::MAX as f64 * 100.0;
            self.playing = true;

            let mut events = services.playback.events();
            self.services = Some(services);
            sender.command(|out, shutdown| {
                shutdown
                    .register(async move {
                        let mut tick = tokio::time::interval(Duration::from_millis(500));
                        loop {
                            tokio::select! {
                                event = events.recv() => match event {
                                    Ok(event) => {
                                        let _ = out.send(DeckUpdate::Playback(event));
                                    }
                                    Err(_) => break,
                                },
                                _ = tick.tick() => {
                                    let _ = out.send(DeckUpdate::Tick);
                                }
                            }
                        }
                    })
                    .drop_on_shutdown()
            });
            return;
        }

        let Some(services) = &self.services else {
            return;
        };
        let playback = &services.playback;

        match action {
            DeckAction::Toggle => playback.toggle(),
            DeckAction::Previous => playback.previous(),
            DeckAction::Next => playback.next(),
            DeckAction::Seek(position) => {
                self.position_ms = position as u32;
                playback.seek(position as u32);
            }
            DeckAction::Volume(percent) => {
                self.volume = percent.clamp(0.0, 100.0);
                playback.set_volume((self.volume / 100.0 * u16::MAX as f64) as u16);
            }
            DeckAction::SetTrack(..) => {}
        }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            DeckUpdate::Playback(event) => match event {
                player::Event::Playing { position_ms, .. } => {
                    self.playing = true;
                    self.position_ms = position_ms;
                }
                player::Event::Paused { position_ms, .. } => {
                    self.playing = false;
                    self.position_ms = position_ms;
                }
                player::Event::Position { position_ms, .. } => self.position_ms = position_ms,
                player::Event::TrackChanged {
                    name, duration_ms, ..
                } => {
                    self.title = name;
                    self.duration_ms = duration_ms;
                    self.position_ms = 0;
                }
                player::Event::Volume { volume } => {
                    self.volume = volume as f64 / u16::MAX as f64 * 100.0;
                }
                player::Event::Stopped => {
                    self.playing = false;
                    self.position_ms = 0;
                }
                _ => {}
            },
            DeckUpdate::Tick => {
                if self.playing {
                    self.position_ms = (self.position_ms + 500).min(self.duration_ms);
                }
            }
        }
    }
}

fn clock(ms: u32) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
