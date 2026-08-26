use std::sync::Arc;

use anyhow::Result;
use gatefold_core::{
    metadata,
    model::PlaylistRef,
    player::{self, Playback},
    session,
    session::Session,
};
use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, adw,
    adw::prelude::*, gtk,
};

use crate::{
    components::{
        deck::{Deck, DeckAction, DeckOutput},
        rack::{Rack, RackAction, RackOutput},
    },
    css,
    pages::{
        home::Home,
        playlist::{PlaylistAction, PlaylistOutput, PlaylistPage},
    },
    palette::Palette,
};

pub struct Services {
    pub session: Session,
    pub playback: Arc<Playback>,
}

pub struct App {
    css: gtk::CssProvider,
    services: Option<Arc<Services>>,
    pages: gtk::Stack,
    rack: Controller<Rack>,
    home: Controller<Home>,
    playlist: Controller<PlaylistPage>,
    deck: Controller<Deck>,
}

#[derive(Debug)]
pub enum AppAction {
    OpenPlaylist(Box<PlaylistRef>),
    OpenHome,
    Cover(std::path::PathBuf),
}

pub enum AppCmd {
    Ready(Arc<Services>),
    Failed(String),
}

impl std::fmt::Debug for AppCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppCmd::Ready(_) => write!(f, "Ready"),
            AppCmd::Failed(error) => write!(f, "Failed({error})"),
        }
    }
}

#[relm4::component(pub)]
impl Component for App {
    type Init = ();
    type Input = AppAction;
    type Output = ();
    type CommandOutput = AppCmd;

    view! {
        adw::ApplicationWindow {
            set_title: Some("gatefold"),
            set_default_size: (1240, 820),

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,

                model.rack.widget() {},

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_hexpand: true,

                    #[local_ref]
                    pages -> gtk::Stack {
                        set_vexpand: true,
                        set_transition_type: gtk::StackTransitionType::Crossfade,
                    },

                    model.deck.widget() {},
                },
            },
        }
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let css = gtk::CssProvider::new();
        css.load_from_string(&css::stylesheet(&Palette::default()));
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().expect("display"),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let model = App {
            css,
            services: None,
            pages: gtk::Stack::new(),
            rack: Rack::builder().launch(()).forward(
                sender.input_sender(),
                |output| match output {
                    RackOutput::OpenPlaylist(playlist) => AppAction::OpenPlaylist(playlist),
                    RackOutput::OpenHome => AppAction::OpenHome,
                },
            ),
            home: Home::builder().launch(()).detach(),
            playlist: PlaylistPage::builder()
                .launch(())
                .forward(sender.input_sender(), |PlaylistOutput::Cover(path)| {
                    AppAction::Cover(path)
                }),
            deck: Deck::builder()
                .launch(())
                .forward(sender.input_sender(), |DeckOutput::Cover(path)| {
                    AppAction::Cover(path)
                }),
        };
        let pages = &model.pages;
        pages.add_named(model.home.widget(), Some("home"));
        pages.add_named(model.playlist.widget(), Some("playlist"));
        let widgets = view_output!();

        sender.oneshot_command(async move {
            match start().await {
                Ok(services) => AppCmd::Ready(services),
                Err(error) => AppCmd::Failed(error.to_string()),
            }
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, action: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            AppAction::Cover(path) => {
                let palette = Palette::from_cover(&path);
                self.css.load_from_string(&css::stylesheet(&palette));
            }
            AppAction::OpenHome => self.pages.set_visible_child_name("home"),
            AppAction::OpenPlaylist(playlist) => {
                let Some(services) = self.services.clone() else {
                    return;
                };
                self.playlist
                    .emit(PlaylistAction::Show(services, *playlist));
                self.pages.set_visible_child_name("playlist");
            }
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
            AppCmd::Ready(services) => {
                self.rack.emit(RackAction::SetServices(services.clone()));
                self.deck.emit(DeckAction::SetServices(services.clone()));
                self.services = Some(services);
                if std::env::var("GATEFOLD_OPEN").is_ok()
                    && let Some(first) = metadata::cached_playlists().into_iter().next()
                {
                    _sender.input(AppAction::OpenPlaylist(Box::new(first)));
                }
            }
            AppCmd::Failed(error) => tracing::error!("{error}"),
        }
    }
}

async fn start() -> Result<Arc<Services>> {
    let session = session::connect().await?;
    tracing::info!("connected as {}", session.username());

    let playback = player::start(session.clone())?;

    Ok(Arc::new(Services { session, playback }))
}
