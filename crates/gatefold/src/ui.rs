use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use gatefold_core::player::Player;
use gatefold_core::{cache_dir, metadata, player, session};
use relm4::adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};

pub struct Gatefold {
    cover: Option<PathBuf>,
    title: String,
    artist: String,
    player: Option<Arc<Player>>,
    playing: bool,
    chrome: bool,
}

pub struct Track {
    cover: PathBuf,
    title: String,
    artist: String,
    player: Arc<Player>,
}

pub enum Loaded {
    Track(Box<Track>),
    Failed(String),
}

impl fmt::Debug for Loaded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Loaded::Track(track) => write!(f, "Track({})", track.title),
            Loaded::Failed(error) => write!(f, "Failed({error})"),
        }
    }
}

#[derive(Debug)]
pub enum Action {
    Toggle,
    Chrome(bool),
}

#[relm4::component(pub)]
impl Component for Gatefold {
    type Init = String;
    type Input = Action;
    type Output = ();
    type CommandOutput = Loaded;

    view! {
        adw::ApplicationWindow {
            set_title: Some("gatefold"),
            set_default_size: (640, 700),

            gtk::WindowHandle {
                gtk::Overlay {
                    gtk::Picture {
                        #[watch]
                        set_filename: model.cover.as_ref(),
                        set_content_fit: gtk::ContentFit::Cover,
                    },

                    add_overlay = &gtk::Revealer {
                        set_valign: gtk::Align::Start,
                        set_transition_type: gtk::RevealerTransitionType::Crossfade,
                        #[watch]
                        set_reveal_child: model.chrome,

                        gtk::Box {
                            add_css_class: "top-scrim",

                            gtk::WindowControls {
                                set_side: gtk::PackType::End,
                                set_hexpand: true,
                                set_halign: gtk::Align::End,
                                add_css_class: "floating",
                            },
                        },
                    },

                    add_overlay = &gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_valign: gtk::Align::End,
                        set_spacing: 16,
                        add_css_class: "scrim",

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_hexpand: true,
                            set_valign: gtk::Align::Center,

                            gtk::Label {
                                #[watch]
                                set_label: &model.title,
                                set_xalign: 0.0,
                                set_ellipsize: relm4::gtk::pango::EllipsizeMode::End,
                                add_css_class: "track-title",
                            },

                            gtk::Label {
                                #[watch]
                                set_label: &model.artist,
                                set_xalign: 0.0,
                                set_ellipsize: relm4::gtk::pango::EllipsizeMode::End,
                                add_css_class: "track-artist",
                            },
                        },

                        gtk::Button {
                            #[watch]
                            set_icon_name: if model.playing {
                                "media-playback-pause-symbolic"
                            } else {
                                "media-playback-start-symbolic"
                            },
                            #[watch]
                            set_sensitive: model.player.is_some(),
                            set_valign: gtk::Align::Center,
                            add_css_class: "play",
                            connect_clicked => Action::Toggle,
                        },
                    },
                },
            },
        }
    }

    fn init(
        uri: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Gatefold {
            cover: None,
            title: String::new(),
            artist: String::new(),
            player: None,
            playing: false,
            chrome: true,
        };
        let widgets = view_output!();

        let motion = gtk::EventControllerMotion::new();
        let enter = sender.input_sender().clone();
        motion.connect_enter(move |_, _, _| enter.emit(Action::Chrome(true)));
        let leave = sender.input_sender().clone();
        motion.connect_leave(move |_| leave.emit(Action::Chrome(false)));
        root.add_controller(motion);

        sender.oneshot_command(async move {
            match load(uri).await {
                Ok(track) => Loaded::Track(Box::new(track)),
                Err(error) => Loaded::Failed(error.to_string()),
            }
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, action: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            Action::Chrome(visible) => self.chrome = visible,
            Action::Toggle => {
                let Some(player) = &self.player else {
                    return;
                };

                if self.playing {
                    player.pause();
                } else {
                    player.play();
                }

                self.playing = !self.playing;
            }
        }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            Loaded::Track(track) => {
                self.cover = Some(track.cover);
                self.title = track.title;
                self.artist = track.artist;
                self.player = Some(track.player);
                self.playing = true;
            }
            Loaded::Failed(error) => tracing::error!("{error}"),
        }
    }
}

async fn load(uri: String) -> Result<Track> {
    let session = session::connect().await?;
    tracing::info!("connected as {}", session.username());

    let track = metadata::track(&session, &uri).await?;
    let artist = track
        .artists
        .iter()
        .map(|artist| artist.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let cover = metadata::cover(&session, &track).await?;
    let path = cache_dir()?.join("cover.jpg");
    std::fs::write(&path, &cover)?;

    let player = player::start(session)?;
    player::load(&player, &uri)?;

    Ok(Track {
        cover: path,
        title: track.name,
        artist,
        player,
    })
}
