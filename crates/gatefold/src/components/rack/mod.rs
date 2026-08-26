use std::{cell::Cell, rc::Rc, sync::Arc};

use gatefold_core::{metadata, model::PlaylistRef};
use relm4::{Component, ComponentParts, ComponentSender, adw, adw::prelude::*, gtk};

use crate::app::Services;

pub const CSS: &str = include_str!("style.css");

const WIDE: i32 = 256;
const NARROW: i32 = 64;
const MARGIN: i32 = 12;
const PAD: i32 = 8;

fn request(visible: i32) -> i32 {
    visible + 2 * MARGIN
}

pub struct Rack {
    services: Option<Arc<Services>>,
    root: gtk::Box,
    inner: gtk::Box,
    scroll: gtk::Adjustment,
    shelf: gtk::Box,
    toggle: gtk::Button,
    avatar: gtk::Label,
    username: gtk::Label,
    wide_static: Vec<gtk::Widget>,
    wide_rows: Vec<gtk::Widget>,
    narrow: Vec<gtk::Widget>,
    collapsed: bool,
    busy: Rc<Cell<bool>>,
}

pub enum RackAction {
    SetServices(Arc<Services>),
    ToggleCollapse,
    Open(String),
}

impl std::fmt::Debug for RackAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RackAction::SetServices(_) => write!(f, "SetServices"),
            RackAction::ToggleCollapse => write!(f, "ToggleCollapse"),
            RackAction::Open(uri) => write!(f, "Open({uri})"),
        }
    }
}

#[derive(Debug)]
pub enum RackOutput {
    OpenPlaylist(String),
}

#[derive(Debug)]
pub enum RackCmd {
    Playlists(Vec<PlaylistRef>),
}

impl Component for Rack {
    type Init = ();
    type Input = RackAction;
    type Output = RackOutput;
    type CommandOutput = RackCmd;
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        let rack = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rack.add_css_class("rack");
        rack.set_overflow(gtk::Overflow::Hidden);
        rack.set_size_request(request(WIDE), -1);
        rack.set_hexpand(false);

        rack
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let inner = gtk::Box::new(gtk::Orientation::Vertical, 0);
        inner.add_css_class("rack-inner");
        inner.set_size_request(WIDE - 2 * PAD, -1);

        let curtain = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::External)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&inner)
            .build();
        if let Some(viewport) = curtain.child().and_downcast::<gtk::Viewport>() {
            viewport.set_scroll_to_focus(false);
        }
        curtain.set_max_content_width(WIDE);
        root.append(&curtain);

        let mut wide_static: Vec<gtk::Widget> = Vec::new();
        let mut narrow: Vec<gtk::Widget> = Vec::new();

        let search = gtk::Entry::new();
        search.add_css_class("search");
        search.set_placeholder_text(Some("Search songs, artists, albums"));
        search.set_primary_icon_name(Some("system-search-symbolic"));
        search.set_width_chars(6);
        wide_static.push(search.clone().upcast());
        inner.append(&search);

        let find = icon("system-search-symbolic", "nav");
        find.set_tooltip_text(Some("Search"));
        find.set_halign(gtk::Align::Start);
        find.set_visible(false);
        narrow.push(find.clone().upcast());
        inner.append(&find);

        let home = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        home.append(&gtk::Image::from_icon_name("go-home-symbolic"));
        let home_label = gtk::Label::new(Some("Home"));
        home_label.set_xalign(0.0);
        home_label.set_hexpand(true);
        wide_static.push(home_label.clone().upcast());
        home.append(&home_label);
        let home = gtk::Button::builder().child(&home).build();
        home.add_css_class("nav");
        home.set_tooltip_text(Some("Home"));
        home.set_margin_top(6);
        inner.append(&home);

        let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        head.add_css_class("shelf-head");
        let title = label("Library", "shelf-title");
        title.set_hexpand(true);
        head.append(&title);
        head.append(&icon("list-add-symbolic", "nudge").upcast::<gtk::Widget>());

        let chips = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        for text in ["Playlists", "Albums", "Artists"] {
            let chip = gtk::Button::with_label(text);
            chip.add_css_class("chip");
            chips.append(&chip);
        }
        let chips = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::External)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .child(&chips)
            .build();
        chips.add_css_class("chips");

        let heading_block = gtk::Box::new(gtk::Orientation::Vertical, 0);
        heading_block.append(&head);
        heading_block.append(&chips);
        let heading = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .transition_duration(260)
            .reveal_child(true)
            .child(&heading_block)
            .build();
        wide_static.push(heading_block.clone().upcast());
        inner.append(&heading);

        let shelf = gtk::Box::new(gtk::Orientation::Vertical, 2);
        shelf.set_margin_top(6);
        let list = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(false)
            .vexpand(true)
            .child(&shelf)
            .build();
        inner.append(&list);

        let utility = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        utility.add_css_class("utility");
        let toggle = icon("sidebar-show-symbolic", "nav");
        toggle.set_tooltip_text(Some("Collapse"));
        toggle.set_hexpand(true);
        toggle.set_halign(gtk::Align::Start);
        let collapse = sender.input_sender().clone();
        toggle.connect_clicked(move |_| collapse.emit(RackAction::ToggleCollapse));
        utility.append(&toggle);
        let settings = icon("emblem-system-symbolic", "nav");
        settings.set_tooltip_text(Some("Settings"));
        wide_static.push(settings.clone().upcast());
        utility.append(&settings);
        inner.append(&utility);

        let account = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        account.add_css_class("account");
        let avatar = gtk::Label::new(Some("·"));
        avatar.add_css_class("avatar");
        account.append(&avatar);
        let who = gtk::Box::new(gtk::Orientation::Vertical, 0);
        who.set_valign(gtk::Align::Center);
        who.set_hexpand(true);
        let username = label("", "shelf-name");
        username.set_ellipsize(gtk::pango::EllipsizeMode::End);
        who.append(&username);
        who.append(&label("Premium", "shelf-kind"));
        wide_static.push(who.clone().upcast());
        account.append(&who);
        inner.append(&account);

        let model = Rack {
            services: None,
            root: root.clone(),
            inner,
            scroll: curtain.hadjustment(),
            shelf,
            toggle,
            avatar: avatar.clone(),
            username: username.clone(),
            wide_static,
            wide_rows: Vec::new(),
            narrow,
            collapsed: false,
            busy: Rc::new(Cell::new(false)),
        };

        let _ = (heading, sender);

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            RackAction::SetServices(services) => {
                let name = services.session.username();
                let initial = name
                    .chars()
                    .next()
                    .unwrap_or('·')
                    .to_uppercase()
                    .to_string();
                self.avatar.set_text(&initial);
                self.username.set_text(&name);
                self.services = Some(services.clone());
                sender.command(|out, shutdown| {
                    shutdown
                        .register(async move {
                            match metadata::playlists(&services.session).await {
                                Ok(playlists) => {
                                    let _ = out.send(RackCmd::Playlists(playlists));
                                }
                                Err(error) => tracing::error!("playlists: {error}"),
                            }
                        })
                        .drop_on_shutdown()
                });
            }
            RackAction::ToggleCollapse => self.toggle_collapse(),
            RackAction::Open(uri) => {
                let _ = sender.output(RackOutput::OpenPlaylist(uri));
            }
        }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            RackCmd::Playlists(playlists) => {
                self.wide_rows.clear();
                while let Some(child) = self.shelf.first_child() {
                    self.shelf.remove(&child);
                }

                for playlist in playlists {
                    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);

                    let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                    tile.add_css_class("tile");
                    tile.set_size_request(40, 40);
                    tile.set_valign(gtk::Align::Center);
                    tile.set_hexpand(false);
                    let glyph = gtk::Image::from_icon_name("emblem-music-symbolic");
                    glyph.set_halign(gtk::Align::Center);
                    glyph.set_hexpand(true);
                    tile.append(&glyph);
                    row.append(&tile);

                    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
                    text.set_valign(gtk::Align::Center);
                    text.set_hexpand(true);
                    let name = label(&playlist.name, "shelf-name");
                    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    text.append(&name);
                    let kind = label(
                        &format!("Playlist · {} songs", playlist.length),
                        "shelf-kind",
                    );
                    kind.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    text.append(&kind);
                    if !self.collapsed {
                        text.set_visible(true);
                    } else {
                        text.set_visible(false);
                    }
                    self.wide_rows.push(text.clone().upcast());
                    row.append(&text);

                    let button = gtk::Button::builder().child(&row).build();
                    button.add_css_class("record");
                    button.set_tooltip_text(Some(&playlist.name));
                    let open = sender.input_sender().clone();
                    let uri = playlist.uri.clone();
                    button.connect_clicked(move |_| open.emit(RackAction::Open(uri.clone())));
                    self.shelf.append(&button);
                }
            }
        }
    }

    fn update_view(&self, _: &mut Self::Widgets, _: ComponentSender<Self>) {}
}

impl Rack {
    fn toggle_collapse(&mut self) {
        if self.busy.replace(true) {
            return;
        }
        self.collapsed = !self.collapsed;
        let collapse = self.collapsed;
        self.toggle
            .set_tooltip_text(Some(if collapse { "Expand" } else { "Collapse" }));

        let (from, to) = if collapse {
            (WIDE, NARROW)
        } else {
            (NARROW, WIDE)
        };
        let wide: Vec<gtk::Widget> = self
            .wide_static
            .iter()
            .chain(self.wide_rows.iter())
            .cloned()
            .collect();

        if !collapse {
            self.inner.set_size_request(WIDE - 2 * PAD, -1);
            for widget in &self.narrow {
                widget.set_visible(false);
            }
            for widget in &wide {
                widget.set_opacity(0.0);
                widget.set_visible(true);
            }
        }

        if let Some(heading) = self
            .wide_static
            .iter()
            .find_map(|widget| widget.parent().and_downcast::<gtk::Revealer>())
        {
            heading.set_reveal_child(!collapse);
        }

        let root = self.root.clone();
        let scroll = self.scroll.clone();
        let fade = wide.clone();
        let target = adw::CallbackAnimationTarget::new(move |value| {
            root.set_size_request(request(value as i32), -1);
            scroll.set_value(0.0);
            let progress = (value - from as f64) / (to - from) as f64;
            let opacity = if collapse {
                1.0 - (progress / 0.6).min(1.0)
            } else {
                ((progress - 0.4) / 0.6).clamp(0.0, 1.0)
            };
            for widget in &fade {
                widget.set_opacity(opacity);
            }
        });

        let animation = adw::TimedAnimation::new(&self.root, from as f64, to as f64, 260, target);
        animation.set_easing(adw::Easing::EaseInOutCubic);
        let (root, inner, busy, narrow) = (
            self.root.clone(),
            self.inner.clone(),
            self.busy.clone(),
            self.narrow.clone(),
        );
        animation.connect_done(move |_| {
            root.set_size_request(request(to), -1);
            if collapse {
                for widget in &wide {
                    widget.set_visible(false);
                }
                for widget in &narrow {
                    widget.set_visible(true);
                }
                inner.set_size_request(NARROW - 2 * PAD, -1);
            }
            for widget in &wide {
                widget.set_opacity(1.0);
            }
            busy.set(false);
        });
        animation.play();
    }
}

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class(class);
    label.set_xalign(0.0);

    label
}

fn icon(name: &str, class: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(name);
    button.add_css_class(class);
    button.set_valign(gtk::Align::Center);

    button
}
