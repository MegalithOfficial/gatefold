use std::sync::Arc;

use anyhow::Result;
use gatefold_core::{
    metadata,
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
        deck::{Deck, DeckAction},
        rack::{Rack, RackAction, RackOutput},
    },
    css,
    pages::home::Home,
    palette::Palette,
};

pub struct Services {
    pub session: Session,
    pub playback: Arc<Playback>,
}

pub struct App {
    services: Option<Arc<Services>>,
    rack: Controller<Rack>,
    home: Controller<Home>,
    deck: Controller<Deck>,
}

#[derive(Debug)]
pub enum AppAction {
    OpenPlaylist(String),
}

pub enum AppCmd {
    Ready(Arc<Services>),
    Queue(Vec<String>),
    Failed(String),
}

impl std::fmt::Debug for AppCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppCmd::Ready(_) => write!(f, "Ready"),
            AppCmd::Queue(uris) => write!(f, "Queue({})", uris.len()),
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

                    model.home.widget() {
                        set_vexpand: true,
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
            services: None,
            rack: Rack::builder()
                .launch(())
                .forward(sender.input_sender(), |RackOutput::OpenPlaylist(uri)| {
                    AppAction::OpenPlaylist(uri)
                }),
            home: Home::builder().launch(()).detach(),
            deck: Deck::builder().launch(()).detach(),
        };
        let widgets = view_output!();

        sender.oneshot_command(async move {
            match start().await {
                Ok(services) => AppCmd::Ready(services),
                Err(error) => AppCmd::Failed(error.to_string()),
            }
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            AppAction::OpenPlaylist(uri) => {
                let Some(services) = self.services.clone() else {
                    return;
                };
                sender.oneshot_command(async move {
                    match metadata::playlist_uris(&services.session, &uri).await {
                        Ok(uris) => AppCmd::Queue(uris),
                        Err(error) => AppCmd::Failed(error.to_string()),
                    }
                });
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
            AppCmd::Ready(services) => {
                self.rack.emit(RackAction::SetServices(services.clone()));
                self.deck.emit(DeckAction::SetServices(services.clone()));
                self.services = Some(services);
            }
            AppCmd::Queue(uris) => {
                if let Some(services) = &self.services {
                    services.playback.play_queue(uris, 0);
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
