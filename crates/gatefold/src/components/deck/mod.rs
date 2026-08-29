use std::{path::PathBuf, sync::Arc, time::Duration};

use gatefold_core::{
    images,
    model::ArtistRef,
    player::{self, Repeat},
};
use relm4::{Component, ComponentParts, ComponentSender, RelmWidgetExt, gtk, gtk::prelude::*};

use crate::{app::Services, artists};

pub const CSS: &str = include_str!("style.css");

#[derive(Debug)]
pub enum DeckOutput {
    Cover(PathBuf),
    OpenArtist(Box<ArtistRef>),
    ToggleLyrics,
}

pub struct Deck {
    services: Option<Arc<Services>>,
    uri: String,
    seeking: bool,
    seek_epoch: u64,
    cover: Option<PathBuf>,
    title: String,
    artists: Vec<ArtistRef>,
    offline: bool,
    playing: bool,
    shuffle: bool,
    repeat: Repeat,
    lyrics_open: bool,
    queue_open: bool,
    queue_sheet: gtk::Popover,
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
    OpenArtist(String),
    Lyrics,
    LyricsOpen(bool),
    Queue,
    QueueOpen(bool),
    QueuePopdown,
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
            DeckAction::OpenArtist(uri) => write!(f, "OpenArtist({uri})"),
            DeckAction::Lyrics => write!(f, "Lyrics"),
            DeckAction::LyricsOpen(open) => write!(f, "LyricsOpen({open})"),
            DeckAction::Queue => write!(f, "Queue"),
            DeckAction::QueueOpen(open) => write!(f, "QueueOpen({open})"),
            DeckAction::QueuePopdown => write!(f, "QueuePopdown"),
        }
    }
}

#[derive(Debug)]
pub enum DeckUpdate {
    Playback(player::Event),
    Cover(String, PathBuf),
    SeekSettle(u64),
    Tick,
}

#[relm4::component(pub)]
impl Component for Deck {
    type Init = gtk::Box;
    type Input = DeckAction;
    type Output = DeckOutput;
    type CommandOutput = DeckUpdate;

    view! {
        gtk::CenterBox {
            add_css_class: "deck",

            #[wrap(Some)]
            set_start_widget = &gtk::Box {
                set_spacing: 12,
                set_valign: gtk::Align::Center,

                gtk::Box {
                    add_css_class: "tile",
                    set_overflow: gtk::Overflow::Hidden,
                    set_valign: gtk::Align::Center,

                    gtk::Image {
                        #[watch]
                        set_from_file: model.cover.as_ref(),
                        #[watch]
                        set_visible: model.cover.is_some(),
                        set_pixel_size: 52,
                    },

                    gtk::Image {
                        set_icon_name: Some("audio-x-generic-symbolic"),
                        #[watch]
                        set_visible: model.cover.is_none(),
                        set_size_request: (52, 52),
                    },
                },

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_valign: gtk::Align::Center,

                    gtk::Label {
                        #[watch]
                        set_label: model.display_title(),
                        #[watch]
                        set_class_active: ("idle", model.title.is_empty()),
                        set_xalign: 0.0,
                        set_width_chars: 8,
                        set_max_width_chars: 16,
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        add_css_class: "now-title",
                    },

                    gtk::Label {
                        #[watch]
                        set_markup: &artists::markup(&model.artists),
                        #[watch]
                        set_visible: !model.artists.is_empty(),
                        #[watch]
                        set_focusable: false,
                        set_xalign: 0.0,
                        set_width_chars: 8,
                        set_max_width_chars: 16,
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        add_css_class: "now-artist",
                        connect_activate_link[sender] => move |_, uri| {
                            sender.input(DeckAction::OpenArtist(uri.to_owned()));
                            gtk::glib::Propagation::Stop
                        },
                    },

                    gtk::Label {
                        set_label: "Reconnecting…",
                        #[watch]
                        set_visible: model.offline,
                        set_xalign: 0.0,
                        add_css_class: "now-offline",
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
                        #[watch]
                        set_class_active: ("active", model.shuffle),
                        #[watch]
                        set_tooltip_text: Some(if model.shuffle {
                            "Shuffle on"
                        } else {
                            "Shuffle off"
                        }),
                        add_css_class: "icon",
                        connect_clicked => DeckAction::Shuffle,

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,

                            gtk::Image {
                                set_icon_name: Some("media-playlist-shuffle-symbolic"),
                            },

                            gtk::Box {
                                add_css_class: "state-dot",
                                set_halign: gtk::Align::Center,
                                #[watch]
                                set_visible: model.shuffle,
                            },
                        },
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
                        set_class_active: ("active", model.repeat != Repeat::Off),
                        #[watch]
                        set_tooltip_text: Some(match model.repeat {
                            Repeat::Off => "Repeat off",
                            Repeat::Context => "Repeat all",
                            Repeat::Track => "Repeat one",
                        }),
                        add_css_class: "icon",
                        connect_clicked => DeckAction::Repeat,

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,

                            gtk::Overlay {
                                gtk::Image {
                                    set_icon_name: Some("media-playlist-repeat-symbolic"),
                                },

                                add_overlay = &gtk::Label {
                                    set_label: "1",
                                    add_css_class: "repeat-one",
                                    set_halign: gtk::Align::End,
                                    set_valign: gtk::Align::Start,
                                    #[watch]
                                    set_visible: model.repeat == Repeat::Track,
                                },
                            },

                            gtk::Box {
                                add_css_class: "state-dot",
                                set_halign: gtk::Align::Center,
                                #[watch]
                                set_visible: model.repeat != Repeat::Off,
                            },
                        },
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

                    #[name = "seek"]
                    gtk::Scale {
                        set_size_request: (440, -1),
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

                #[name = "queue_button"]
                gtk::Button {
                    set_icon_name: "view-list-symbolic",
                    set_tooltip_text: Some("Queue"),
                    #[watch]
                    set_class_active: ("active", model.queue_open),
                    add_css_class: "icon",
                    connect_clicked => DeckAction::Queue,
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
                    set_icon_name: "gatefold-lyrics-symbolic",
                    set_tooltip_text: Some("Lyrics"),
                    #[watch]
                    set_class_active: ("active", model.lyrics_open),
                    add_css_class: "icon",
                    connect_clicked => DeckAction::Lyrics,
                },
            },
        }
    }

    fn init(
        queue_sheet: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let popover = gtk::Popover::new();
        popover.set_has_arrow(false);
        popover.set_position(gtk::PositionType::Top);
        popover.set_offset(0, -8);
        popover.remove_css_class("background");
        popover.add_css_class("quick-menu");
        popover.set_child(Some(&queue_sheet));
        let model = Deck {
            services: None,
            uri: String::new(),
            seeking: false,
            seek_epoch: 0,
            cover: None,
            title: String::new(),
            artists: Vec::new(),
            offline: false,
            playing: false,
            shuffle: false,
            repeat: Repeat::Off,
            lyrics_open: false,
            queue_open: false,
            queue_sheet: popover,
            position_ms: 0,
            duration_ms: 0,
            volume: 100.0,
        };
        let widgets = view_output!();
        model.queue_sheet.set_parent(&widgets.queue_button);
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
                self.seeking = true;
                self.seek_epoch += 1;
                let epoch = self.seek_epoch;
                sender.oneshot_command(async move {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    DeckUpdate::SeekSettle(epoch)
                });
            }
            DeckAction::Volume(percent) => {
                self.volume = percent.clamp(0.0, 100.0);
                playback.set_volume((self.volume / 100.0 * u16::MAX as f64) as u16);
            }
            DeckAction::OpenArtist(uri) => {
                if let Some(artist) = self.artists.iter().find(|artist| artist.uri == uri) {
                    let _ = sender.output(DeckOutput::OpenArtist(Box::new(artist.clone())));
                }
            }
            DeckAction::Lyrics => {
                let _ = sender.output(DeckOutput::ToggleLyrics);
            }
            DeckAction::LyricsOpen(open) => self.lyrics_open = open,
            DeckAction::Queue => self.queue_sheet.popup(),
            DeckAction::QueueOpen(open) => self.queue_open = open,
            DeckAction::QueuePopdown => self.queue_sheet.popdown(),
            DeckAction::SetServices(_) => {}
        }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        let _sender = _sender;
        match message {
            DeckUpdate::Playback(event) => match event {
                player::Event::Loading { uri } => self.uri = uri,
                player::Event::Playing { uri, position_ms } => {
                    self.playing = true;
                    if uri == self.uri && !self.seeking {
                        self.position_ms = position_ms;
                    }
                }
                player::Event::Paused { uri, position_ms } => {
                    self.playing = false;
                    if uri == self.uri && !self.seeking {
                        self.position_ms = position_ms;
                    }
                }
                player::Event::Position { uri, position_ms } => {
                    if uri == self.uri && !self.seeking {
                        self.position_ms = position_ms;
                    }
                }
                player::Event::Connection { online } => self.offline = !online,
                player::Event::TrackChanged {
                    uri,
                    name,
                    artists,
                    duration_ms,
                    cover_id,
                } => {
                    self.uri = uri.clone();
                    self.title = name;
                    self.artists = artists;
                    self.duration_ms = duration_ms;
                    self.position_ms = 0;
                    if let Some(id) = cover_id {
                        if let Some(path) = images::cached(&id) {
                            self.cover = Some(path.clone());
                            let _ = _sender.output(DeckOutput::Cover(path));
                        } else if let Some(services) = self.services.clone() {
                            _sender.oneshot_command(async move {
                                match images::fetch(&services.session(), &id).await {
                                    Ok(path) => DeckUpdate::Cover(uri, path),
                                    Err(error) => {
                                        tracing::warn!("cover: {error}");
                                        DeckUpdate::Tick
                                    }
                                }
                            });
                        }
                    }
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
            DeckUpdate::Cover(uri, path) => {
                if uri == self.uri {
                    self.cover = Some(path.clone());
                    let _ = _sender.output(DeckOutput::Cover(path));
                }
            }
            DeckUpdate::SeekSettle(epoch) => {
                if self.seeking && epoch == self.seek_epoch {
                    self.seeking = false;
                    if let Some(services) = &self.services {
                        services.playback.seek(self.position_ms);
                    }
                }
            }
            DeckUpdate::Tick => {
                if self.playing && !self.seeking {
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
