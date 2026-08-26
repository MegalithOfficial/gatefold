use std::path::PathBuf;

use relm4::{Component, ComponentParts, ComponentSender, gtk, gtk::prelude::*};

pub const CSS: &str = include_str!("style.css");

pub struct NowPlaying {
    cover: Option<PathBuf>,
    chrome: bool,
}

#[derive(Debug)]
pub enum NowPlayingAction {
    SetCover(PathBuf),
    Chrome(bool),
}

#[relm4::component(pub)]
impl Component for NowPlaying {
    type Init = ();
    type Input = NowPlayingAction;
    type Output = ();
    type CommandOutput = ();

    view! {
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
            },
        }
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = NowPlaying {
            cover: None,
            chrome: true,
        };
        let widgets = view_output!();

        let motion = gtk::EventControllerMotion::new();
        let enter = sender.input_sender().clone();
        motion.connect_enter(move |_, _, _| enter.emit(NowPlayingAction::Chrome(true)));
        let leave = sender.input_sender().clone();
        motion.connect_leave(move |_| leave.emit(NowPlayingAction::Chrome(false)));
        root.add_controller(motion);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, action: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            NowPlayingAction::SetCover(cover) => self.cover = Some(cover),
            NowPlayingAction::Chrome(visible) => self.chrome = visible,
        }
    }
}
