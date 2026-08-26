use relm4::{Component, ComponentParts, ComponentSender, gtk, gtk::prelude::*};

pub const CSS: &str = include_str!("style.css");

pub struct Home {}

#[relm4::component(pub)]
impl Component for Home {
    type Init = ();
    type Input = ();
    type Output = ();
    type CommandOutput = ();

    view! {
        gtk::Box {
            add_css_class: "home",
            set_hexpand: true,
            set_vexpand: true,
        }
    }

    fn init(
        _: Self::Init,
        _root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Home {};
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }
}
