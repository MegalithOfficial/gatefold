use std::{path::PathBuf, sync::Arc, time::Duration};

use gatefold_core::player::{self, Repeat};
use relm4::{Component, ComponentParts, ComponentSender, RelmWidgetExt, gtk, gtk::prelude::*};

use crate::app::Services;

pub const CSS: &str = include_str!("style.css");

pub struct Deck {
    services: Option<Arc<Services>>,
    cover: Option<PathBuf>,
    title: String,
    artist: String,
    playing: bool,
    shuffle: bool,
    repeat: Repeat,
    position_ms: u32,
    duration_ms: u32,
    volume: f64,
}

pub enum DeckAction {
    SetServices(Arc<Services>),
    Toggle,
    Previous,
    Next,
    Shuffle,
    Repeat,
    Seek(f64),
    Volume(f64),
}

impl std::fmt::Debug for DeckAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeckAction::SetServices(_) => write!(f, "SetServices"),
            DeckAction::Toggle => write!(f, "Toggle"),
            DeckAction::Previous => write!(f, "Previous"),
            DeckAction::Next => write!(f, "Next"),
            DeckAction::Shuffle => write!(f, "Shuffle"),
            DeckAction::Repeat => write!(f, "Repeat"),
            DeckAction::Seek(value) => write!(f, "Seek({value})"),
            DeckAction::Volume(value) => write!(f, "Volume({value})"),
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
                    #[watch]
                    set_visible: model.cover.is_some(),
                    set_pixel_size: 52,
                    add_css_class: "thumb",
                },

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_valign: gtk::Align::Center,

                    gtk::Label {
                        #[watch]
                        set_label: model.display_title(),
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
                        set_icon_name: "media-playlist-shuffle-symbolic",
                        #[watch]
                        set_class_active: ("active", model.shuffle),
                        add_css_class: "icon",
                        connect_clicked => DeckAction::Shuffle,
                    },

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

                    gtk::Button {
                        #[watch]
                        set_icon_name: if model.repeat == Repeat::Track {
                            "media-playlist-repeat-song-symbolic"
                        } else {
                            "media-playlist-repeat-symbolic"
                        },
                        #[watch]
                        set_class_active: ("active", model.repeat != Repeat::Off),
                        add_css_class: "icon",
                        connect_clicked => DeckAction::Repeat,
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
                set_spacing: 2,
                set_valign: gtk::Align::Center,

                gtk::Button {
                    set_icon_name: "view-list-symbolic",
                    set_tooltip_text: Some("Queue"),
                    add_css_class: "icon",
                },

                gtk::Box {
                    add_css_class: "volume",
                    set_spacing: 6,
                    set_valign: gtk::Align::Center,
                    set_margin_start: 6,
                    set_margin_end: 6,

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

                gtk::Button {
                    set_icon_name: "view-fullscreen-symbolic",
                    set_tooltip_text: Some("Full screen"),
                    add_css_class: "icon",
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
            shuffle: false,
            repeat: Repeat::Off,
            position_ms: 0,
            duration_ms: 0,
            volume: 100.0,
        };
        let widgets = view_output!();
        let _ = (root, sender);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        if let DeckAction::SetServices(services) = action {
            self.volume = services.playback.volume() as f64 / u16::MAX as f64 * 100.0;
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
            DeckAction::Shuffle => playback.set_shuffle(!self.shuffle),
            DeckAction::Repeat => playback.set_repeat(match self.repeat {
                Repeat::Off => Repeat::Context,
                Repeat::Context => Repeat::Track,
                Repeat::Track => Repeat::Off,
            }),
            DeckAction::Seek(position) => {
                self.position_ms = position as u32;
                playback.seek(position as u32);
            }
            DeckAction::Volume(percent) => {
                self.volume = percent.clamp(0.0, 100.0);
                playback.set_volume((self.volume / 100.0 * u16::MAX as f64) as u16);
            }
            DeckAction::SetServices(_) => {}
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
                    name,
                    artists,
                    duration_ms,
                    ..
                } => {
                    self.title = name;
                    self.artist = artists;
                    self.duration_ms = duration_ms;
                    self.position_ms = 0;
                }
                player::Event::ShuffleChanged { shuffle } => self.shuffle = shuffle,
                player::Event::RepeatChanged { repeat } => self.repeat = repeat,
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

impl Deck {
    fn display_title(&self) -> &str {
        if self.title.is_empty() {
            "Nothing playing"
        } else {
            &self.title
        }
    }
}

fn clock(ms: u32) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
