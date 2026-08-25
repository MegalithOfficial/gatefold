use std::path::PathBuf;

use anyhow::Result;
use gatefold_core::{cache_dir, metadata, player, session};
use relm4::adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};

pub struct Gatefold {
    cover: Option<PathBuf>,
}

#[derive(Debug)]
pub enum Loaded {
    Cover(PathBuf),
    Failed(String),
}

#[relm4::component(pub)]
impl Component for Gatefold {
    type Init = String;
    type Input = ();
    type Output = ();
    type CommandOutput = Loaded;

    view! {
        adw::ApplicationWindow {
            set_title: Some("gatefold"),
            set_default_size: (640, 720),

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,

                adw::HeaderBar {},

                adw::Spinner {
                    #[watch]
                    set_visible: model.cover.is_none(),
                    set_vexpand: true,
                },

                gtk::Picture {
                    #[watch]
                    set_visible: model.cover.is_some(),
                    #[watch]
                    set_filename: model.cover.as_ref(),
                    set_content_fit: gtk::ContentFit::Contain,
                    set_vexpand: true,
                },
            },
        }
    }

    fn init(
        uri: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Gatefold { cover: None };
        let widgets = view_output!();

        sender.oneshot_command(async move {
            match load(uri).await {
                Ok(cover) => Loaded::Cover(cover),
                Err(error) => Loaded::Failed(error.to_string()),
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
            Loaded::Cover(cover) => self.cover = Some(cover),
            Loaded::Failed(error) => tracing::error!("{error}"),
        }
    }
}

async fn load(uri: String) -> Result<PathBuf> {
    let session = session::connect().await?;
    tracing::info!("connected as {}", session.username());

    let track = metadata::track(&session, &uri).await?;
    let artists: Vec<&str> = track.artists.iter().map(|a| a.name.as_str()).collect();
    tracing::info!("{} by {} ({})", track.name, artists.join(", "), track.album.name);

    let cover = metadata::cover(&session, &track).await?;
    let path = cache_dir()?.join("cover.jpg");
    std::fs::write(&path, &cover)?;

    relm4::spawn(async move {
        if let Err(error) = player::play(session, &uri).await {
            tracing::error!("playback failed: {error}");
        }
    });

    Ok(path)
}
