use std::path::PathBuf;

use relm4::adw::prelude::*;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};

pub struct Gatefold {
    cover: PathBuf,
}

#[relm4::component(pub)]
impl SimpleComponent for Gatefold {
    type Init = PathBuf;
    type Input = ();
    type Output = ();

    view! {
        adw::ApplicationWindow {
            set_title: Some("gatefold"),
            set_default_size: (640, 720),

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,

                adw::HeaderBar {},

                gtk::Picture {
                    set_filename: Some(&model.cover),
                    set_content_fit: gtk::ContentFit::Contain,
                    set_vexpand: true,
                },
            },
        }
    }

    fn init(
        cover: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Gatefold { cover };
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }
}
