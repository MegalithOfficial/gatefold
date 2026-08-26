use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use relm4::{Component, ComponentParts, ComponentSender, adw, adw::prelude::*, gtk};

pub const CSS: &str = include_str!("style.css");

const MAX_SEARCH: i32 = 560;

pub struct Topbar {
    can_back: bool,
    can_forward: bool,
    search: gtk::Entry,
}

#[derive(Debug)]
pub enum TopbarAction {
    History { back: bool, forward: bool },
    ToggleRack,
    Back,
    Forward,
    FocusSearch,
}

#[derive(Debug)]
pub enum TopbarOutput {
    ToggleRack,
    Back,
    Forward,
}

#[relm4::component(pub)]
impl Component for Topbar {
    type Init = ();
    type Input = TopbarAction;
    type Output = TopbarOutput;
    type CommandOutput = ();

    view! {
        gtk::Box {
            add_css_class: "topbar",
            set_spacing: 4,

            gtk::Button {
                set_icon_name: "gatefold-sidebar-symbolic",
                set_tooltip_text: Some("Toggle sidebar"),
                add_css_class: "nav-arrow",
                set_valign: gtk::Align::Center,
                set_margin_end: 6,
                connect_clicked => TopbarAction::ToggleRack,
            },

            gtk::Button {
                set_icon_name: "go-previous-symbolic",
                set_tooltip_text: Some("Back"),
                add_css_class: "nav-arrow",
                set_valign: gtk::Align::Center,
                #[watch]
                set_sensitive: model.can_back,
                connect_clicked => TopbarAction::Back,
            },

            gtk::Button {
                set_icon_name: "go-next-symbolic",
                set_tooltip_text: Some("Forward"),
                add_css_class: "nav-arrow",
                set_valign: gtk::Align::Center,
                #[watch]
                set_sensitive: model.can_forward,
                connect_clicked => TopbarAction::Forward,
            },

            #[local_ref]
            search -> gtk::Entry {
                add_css_class: "search",
                set_placeholder_text: Some("Search songs, artists, albums"),
                set_primary_icon_name: Some("system-search-symbolic"),
                set_tooltip_text: Some("Search (Ctrl+K)"),
                set_width_chars: 38,
                set_margin_start: 6,
                set_valign: gtk::Align::Center,
            },

            #[name = "spacer"]
            gtk::Box {
                set_hexpand: true,
            },

            gtk::WindowControls {
                set_side: gtk::PackType::End,
                set_valign: gtk::Align::Center,
                add_css_class: "floating",
            },
        }
    }

    fn init(
        _: Self::Init,
        _root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let search = gtk::Entry::new();
        let model = Topbar {
            can_back: false,
            can_forward: false,
            search: search.clone(),
        };
        let widgets = view_output!();

        search.connect_changed(|search| {
            let empty = search.text().is_empty();
            search.set_secondary_icon_name((!empty).then_some("edit-clear-symbolic"));
        });

        let idle = Rc::new(Cell::new(0));
        let running: Rc<RefCell<Option<adw::TimedAnimation>>> = Rc::new(RefCell::new(None));
        let glide = {
            let running = running.clone();
            move |widget: gtk::Widget, from: i32, to: i32| {
                if let Some(previous) = running.borrow_mut().take() {
                    previous.skip();
                }
                let target = adw::CallbackAnimationTarget::new({
                    let widget = widget.clone();
                    move |value| widget.set_size_request(value as i32, -1)
                });
                let animation =
                    adw::TimedAnimation::new(&widget, from as f64, to as f64, 220, target);
                animation.set_easing(adw::Easing::EaseOutCubic);
                animation.play();
                *running.borrow_mut() = Some(animation);
            }
        };

        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter({
            let search = search.clone();
            let spacer = widgets.spacer.clone();
            let idle = idle.clone();
            let glide = glide.clone();
            move |_| {
                if idle.get() == 0 {
                    idle.set(search.width());
                }
                let open = spacer.width();
                let growth = (MAX_SEARCH - search.width()).max(0);
                spacer.set_hexpand(false);
                search.set_size_request(-1, -1);
                search.set_hexpand(true);
                glide(spacer.clone().upcast(), open, (open - growth).max(12));
            }
        });
        focus.connect_leave({
            let search = search.clone();
            let spacer = widgets.spacer.clone();
            let idle = idle.clone();
            let glide = glide.clone();
            move |_| {
                let from = search.width();
                search.set_hexpand(false);
                search.set_size_request(from, -1);
                spacer.set_hexpand(true);
                spacer.set_size_request(-1, -1);
                let to = if idle.get() > 0 { idle.get() } else { from };
                glide(search.clone().upcast(), from, to);
            }
        });
        search.add_controller(focus);

        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let search = search.clone();
            move |_, key, _, _| {
                if key == gtk::gdk::Key::Escape {
                    if let Some(root) = search.root() {
                        root.set_focus(None::<&gtk::Widget>);
                    }
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            }
        });
        search.add_controller(keys);
        search.connect_icon_release(|search, position| {
            if position == gtk::EntryIconPosition::Secondary {
                search.set_text("");
            }
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            TopbarAction::History { back, forward } => {
                self.can_back = back;
                self.can_forward = forward;
            }
            TopbarAction::ToggleRack => {
                let _ = sender.output(TopbarOutput::ToggleRack);
            }
            TopbarAction::Back => {
                let _ = sender.output(TopbarOutput::Back);
            }
            TopbarAction::Forward => {
                let _ = sender.output(TopbarOutput::Forward);
            }
            TopbarAction::FocusSearch => {
                self.search.grab_focus();
            }
        }
    }
}
