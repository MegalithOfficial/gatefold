use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use gatefold_core::{
    cache_dir, metadata,
    player::{self, Playback},
    session,
};
use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, adw,
    adw::prelude::*, gtk,
};

use crate::{
    components::deck::{Deck, DeckAction},
    pages::now_playing::{NowPlaying, NowPlayingAction},
    palette::Palette,
};

pub struct Services {
    pub playback: Arc<Playback>,
}

pub struct App {
    css: gtk::CssProvider,
    now_playing: Controller<NowPlaying>,
    deck: Controller<Deck>,
}

pub struct Track {
    pub cover: PathBuf,
    pub title: String,
    pub artist: String,
    pub duration_ms: u32,
}

pub enum Startup {
    Ready(Arc<Services>, Track),
    Failed(String),
}

impl std::fmt::Debug for Startup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Startup::Ready(_, track) => write!(f, "Ready({})", track.title),
            Startup::Failed(error) => write!(f, "Failed({error})"),
        }
    }
}

#[relm4::component(pub)]
impl Component for App {
    type Init = String;
    type Input = ();
    type Output = ();
    type CommandOutput = Startup;

    view! {
        adw::ApplicationWindow {
            set_title: Some("gatefold"),
            set_default_size: (640, 780),

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,

                model.now_playing.widget() {
                    set_vexpand: true,
                },

                model.deck.widget() {},
            },
        }
    }

    fn init(
        uri: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let css = gtk::CssProvider::new();
        css.load_from_string(&crate::css::stylesheet(&Palette::default()));
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().expect("display"),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let model = App {
            css,
            now_playing: NowPlaying::builder().launch(()).detach(),
            deck: Deck::builder().launch(()).detach(),
        };
        let widgets = view_output!();

        sender.oneshot_command(async move {
            match start(uri).await {
                Ok((services, track)) => Startup::Ready(services, track),
                Err(error) => Startup::Failed(error.to_string()),
            }
        });

        ComponentParts { model, widgets }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            Startup::Ready(services, track) => {
                let palette = Palette::from_cover(&track.cover);
                self.css.load_from_string(&crate::css::stylesheet(&palette));

                self.now_playing
                    .emit(NowPlayingAction::SetCover(track.cover.clone()));
                self.deck.emit(DeckAction::SetTrack(services, track));
            }
            Startup::Failed(error) => tracing::error!("{error}"),
        }
    }
}

async fn start(uri: String) -> Result<(Arc<Services>, Track)> {
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

    let playback = player::start(session)?;
    playback.play_queue(vec![uri], 0);

    Ok((
        Arc::new(Services { playback }),
        Track {
            cover: path,
            title: track.name,
            artist,
            duration_ms: track.duration.max(0) as u32,
        },
    ))
}
