use std::{cell::Cell, rc::Rc, sync::Arc};

use gatefold_core::{
    images, metadata,
    model::{PlaylistRef, Profile},
    session,
};
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
    avatar: gtk::Label,
    portrait: gtk::Picture,
    username: gtk::Label,
    menu_name: gtk::Label,
    menu_avatar: gtk::Label,
    menu_portrait: gtk::Picture,
    wide_static: Vec<gtk::Widget>,
    wide_rows: Vec<gtk::Widget>,
    thumbs: Vec<(String, gtk::Image)>,
    narrow: Vec<gtk::Widget>,
    collapsed: bool,
    busy: Rc<Cell<bool>>,
}

pub enum RackAction {
    SetServices(Arc<Services>),
    ShowCached,
    ShowAvatar(std::path::PathBuf),
    ToggleCollapse,
    Home,
    Open(Box<PlaylistRef>),
    AddAccount,
    ConnectSpotify,
    LogOut,
}

impl std::fmt::Debug for RackAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RackAction::SetServices(_) => write!(f, "SetServices"),
            RackAction::ShowCached => write!(f, "ShowCached"),
            RackAction::ShowAvatar(_) => write!(f, "ShowAvatar"),
            RackAction::ToggleCollapse => write!(f, "ToggleCollapse"),
            RackAction::Home => write!(f, "Home"),
            RackAction::Open(playlist) => write!(f, "Open({})", playlist.name),
            RackAction::AddAccount => write!(f, "AddAccount"),
            RackAction::ConnectSpotify => write!(f, "ConnectSpotify"),
            RackAction::LogOut => write!(f, "LogOut"),
        }
    }
}

#[derive(Debug)]
pub enum RackOutput {
    OpenPlaylist(Box<PlaylistRef>),
    OpenHome,
    AddAccount,
    LogOut,
}

#[derive(Debug)]
pub enum RackCmd {
    Playlists(Vec<PlaylistRef>),
    Picture(String, std::path::PathBuf),
    Profile(Profile),
    Avatar(std::path::PathBuf),
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
        let narrow: Vec<gtk::Widget> = Vec::new();

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
        let on_home = sender.input_sender().clone();
        home.connect_clicked(move |_| on_home.emit(RackAction::Home));
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
        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        wheel.connect_scroll({
            let vertical = list.vadjustment();
            move |controller, _, dy| {
                if controller.unit() != gtk::gdk::ScrollUnit::Wheel {
                    return gtk::glib::Propagation::Proceed;
                }
                let row = 54.0;
                let target = ((vertical.value() + dy * 2.0 * row) / row).round() * row;
                vertical.set_value(
                    target.clamp(vertical.lower(), vertical.upper() - vertical.page_size()),
                );
                gtk::glib::Propagation::Stop
            }
        });
        list.add_controller(wheel);
        inner.append(&list);

        let account = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        account.add_css_class("account");
        let identity = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let (frame, avatar, portrait) = portrait_frame(40);
        identity.append(&frame);
        let who = gtk::Box::new(gtk::Orientation::Vertical, 0);
        who.set_valign(gtk::Align::Center);
        who.set_hexpand(true);
        let username = label("", "shelf-name");
        username.set_ellipsize(gtk::pango::EllipsizeMode::End);
        who.append(&username);
        who.append(&label("Spotify Premium", "shelf-kind"));
        wide_static.push(who.clone().upcast());
        identity.append(&who);
        let me = gtk::Button::builder().child(&identity).build();
        me.add_css_class("me");
        me.set_hexpand(true);
        me.set_focus_on_click(false);
        me.set_tooltip_text(Some("Account"));
        account.append(&me);
        let settings = icon("emblem-system-symbolic", "nudge");
        settings.set_tooltip_text(Some("Settings"));
        settings.set_valign(gtk::Align::Center);
        wide_static.push(settings.clone().upcast());
        account.append(&settings);
        inner.append(&account);

        let menu = gtk::Popover::new();
        menu.set_parent(&me);
        menu.set_position(gtk::PositionType::Top);
        menu.set_has_arrow(false);
        menu.set_offset(0, -6);
        menu.remove_css_class("background");
        menu.add_css_class("quick-menu");
        let sheet = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sheet.set_width_request(240);
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        head.add_css_class("menu-head");
        let (menu_frame, menu_avatar, menu_portrait) = portrait_frame(36);
        head.append(&menu_frame);
        let me_text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        me_text.set_valign(gtk::Align::Center);
        me_text.set_hexpand(true);
        let menu_name = label("", "quick-name");
        menu_name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        me_text.append(&menu_name);
        me_text.append(&label("Spotify Premium", "quick-sub"));
        head.append(&me_text);
        let active = gtk::Image::from_icon_name("object-select-symbolic");
        active.add_css_class("menu-check");
        head.append(&active);
        sheet.append(&head);
        let add_account = menu_row(&menu_icon("list-add-symbolic"), "Add account", None);
        add_account.connect_clicked({
            let menu = menu.clone();
            let sender = sender.input_sender().clone();
            move |_| {
                menu.popdown();
                sender.emit(RackAction::AddAccount);
            }
        });
        sheet.append(&add_account);
        let log_out = menu_row(&menu_icon("system-log-out-symbolic"), "Log out", None);
        log_out.set_margin_top(6);
        log_out.connect_clicked({
            let menu = menu.clone();
            let sender = sender.input_sender().clone();
            move |_| {
                menu.popdown();
                sender.emit(RackAction::LogOut);
            }
        });
        sheet.append(&log_out);
        menu.set_child(Some(&sheet));
        for trigger in [&me, &settings] {
            let menu = menu.clone();
            trigger.connect_clicked(move |_| menu.popup());
        }

        let model = Rack {
            services: None,
            root: root.clone(),
            inner,
            scroll: curtain.hadjustment(),
            shelf,
            avatar: avatar.clone(),
            portrait: portrait.clone(),
            username: username.clone(),
            menu_name: menu_name.clone(),
            menu_avatar: menu_avatar.clone(),
            menu_portrait: menu_portrait.clone(),
            wide_static,
            wide_rows: Vec::new(),
            thumbs: Vec::new(),
            narrow,
            collapsed: false,
            busy: Rc::new(Cell::new(false)),
        };

        let _ = heading;
        sender.input(RackAction::ShowCached);

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            RackAction::SetServices(services) => {
                self.services = Some(services.clone());
                let profile_session = services.session.clone();
                sender.command(|out, shutdown| {
                    shutdown
                        .register(async move {
                            match session::profile(&profile_session).await {
                                Ok(profile) => {
                                    let _ = out.send(RackCmd::Profile(profile));
                                }
                                Err(error) => tracing::error!("profile: {error}"),
                            }
                        })
                        .drop_on_shutdown()
                });
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
            RackAction::ShowCached => {
                let cached = metadata::cached_playlists();
                if !cached.is_empty() {
                    self.render(cached, &sender);
                }
                if let Some(profile) = session::cached_profile() {
                    self.apply_profile(profile, &sender);
                }
            }
            RackAction::ShowAvatar(path) => self.show_portrait(&path),
            RackAction::ToggleCollapse => self.toggle_collapse(),
            RackAction::Home => {
                let _ = sender.output(RackOutput::OpenHome);
            }
            RackAction::Open(playlist) => {
                let _ = sender.output(RackOutput::OpenPlaylist(playlist));
            }
            RackAction::AddAccount => self.add_account(&sender),
            RackAction::ConnectSpotify => {
                let _ = sender.output(RackOutput::AddAccount);
            }
            RackAction::LogOut => {
                let _ = sender.output(RackOutput::LogOut);
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
            RackCmd::Playlists(playlists) => self.render(playlists, &sender),
            RackCmd::Profile(profile) => self.apply_profile(profile, &sender),
            RackCmd::Avatar(path) => self.show_portrait(&path),
            RackCmd::Picture(uri, path) => {
                for (row_uri, image) in &self.thumbs {
                    if row_uri == &uri {
                        image.set_from_file(Some(&path));
                        image.set_pixel_size(40);
                        if let Some(tile) = image.parent() {
                            tile.remove_css_class("tile");
                            tile.add_css_class("thumb-frame");
                        }
                    }
                }
            }
        }
    }

    fn update_view(&self, _: &mut Self::Widgets, _: ComponentSender<Self>) {}
}

impl Rack {
    fn add_account(&self, sender: &ComponentSender<Self>) {
        let dialog = adw::Dialog::new();
        dialog.add_css_class("add-account");
        dialog.set_title("Add account");
        dialog.set_content_width(320);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 2);
        body.add_css_class("sheet-body");
        body.append(&label("Add account", "sheet-title"));
        let sub = label(
            "Sign in to another service and switch between them from the sidebar.",
            "sheet-sub",
        );
        sub.set_wrap(true);
        sub.set_margin_bottom(14);
        body.append(&sub);
        let spotify = provider_row(
            "S",
            "Spotify",
            "Sign in with your Spotify account",
            &menu_icon("go-next-symbolic"),
        );
        spotify.connect_clicked({
            let dialog = dialog.clone();
            let sender = sender.input_sender().clone();
            move |_| {
                dialog.close();
                sender.emit(RackAction::ConnectSpotify);
            }
        });
        body.append(&spotify);
        let youtube = provider_row(
            "Y",
            "YouTube Music",
            "Not available yet",
            &label("Coming soon", "menu-note"),
        );
        youtube.set_sensitive(false);
        body.append(&youtube);
        dialog.set_child(Some(&body));
        dialog.present(Some(&self.root));
    }

    fn show_portrait(&self, path: &std::path::Path) {
        for (portrait, initial) in [
            (&self.portrait, &self.avatar),
            (&self.menu_portrait, &self.menu_avatar),
        ] {
            portrait.set_filename(Some(path));
            portrait.set_visible(true);
            initial.set_visible(false);
        }
    }

    fn apply_profile(&mut self, profile: Profile, sender: &ComponentSender<Self>) {
        let initial = profile
            .name
            .chars()
            .next()
            .unwrap_or('·')
            .to_uppercase()
            .to_string();
        self.avatar.set_text(&initial);
        self.menu_avatar.set_text(&initial);
        self.username.set_text(&profile.name);
        self.menu_name.set_text(&profile.name);

        let Some(avatar) = profile.avatar else {
            return;
        };
        if let Some(path) = images::cached(&avatar) {
            sender.input_sender().emit(RackAction::ShowAvatar(path));
        } else if let Some(services) = self.services.clone() {
            sender.command(move |out, shutdown| {
                shutdown
                    .register(async move {
                        if let Ok(path) = images::fetch(&services.session, &avatar).await {
                            let _ = out.send(RackCmd::Avatar(path));
                        }
                    })
                    .drop_on_shutdown()
            });
        }
    }

    fn render(&mut self, playlists: Vec<PlaylistRef>, sender: &ComponentSender<Self>) {
        self.wide_rows.clear();
        self.thumbs.clear();
        while let Some(child) = self.shelf.first_child() {
            self.shelf.remove(&child);
        }

        for playlist in playlists {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);

            let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            tile.set_size_request(40, 40);
            tile.set_valign(gtk::Align::Center);
            tile.set_hexpand(false);
            tile.set_overflow(gtk::Overflow::Hidden);
            let cached = playlist.picture.as_deref().and_then(images::cached);
            let glyph = match &cached {
                Some(path) => {
                    tile.add_css_class("thumb-frame");
                    let image = gtk::Image::from_file(path);
                    image.set_pixel_size(40);
                    image
                }
                None => {
                    tile.add_css_class("tile");
                    gtk::Image::from_icon_name("audio-x-generic-symbolic")
                }
            };
            glyph.set_halign(gtk::Align::Center);
            glyph.set_hexpand(true);
            tile.append(&glyph);
            row.append(&tile);
            self.thumbs.push((playlist.uri.clone(), glyph));

            if cached.is_none()
                && let (Some(picture), Some(services)) = (&playlist.picture, &self.services)
            {
                let (services, id, uri) = (services.clone(), picture.clone(), playlist.uri.clone());
                sender.command(move |out, shutdown| {
                    shutdown
                        .register(async move {
                            if let Ok(path) = images::fetch(&services.session, &id).await {
                                let _ = out.send(RackCmd::Picture(uri, path));
                            }
                        })
                        .drop_on_shutdown()
                });
            }

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
            text.set_visible(!self.collapsed);
            self.wide_rows.push(text.clone().upcast());
            row.append(&text);

            let button = gtk::Button::builder().child(&row).build();
            button.add_css_class("record");
            button.set_tooltip_text(Some(&playlist.name));
            let open = sender.input_sender().clone();
            let entry = playlist.clone();
            button.connect_clicked(move |_| open.emit(RackAction::Open(Box::new(entry.clone()))));
            self.shelf.append(&button);
        }
    }

    fn toggle_collapse(&mut self) {
        if self.busy.replace(true) {
            return;
        }
        self.collapsed = !self.collapsed;
        let collapse = self.collapsed;

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

fn menu_icon(name: &str) -> gtk::Image {
    let image = gtk::Image::from_icon_name(name);
    image.add_css_class("menu-icon");

    image
}

fn portrait_frame(size: i32) -> (gtk::Overlay, gtk::Label, gtk::Picture) {
    let frame = gtk::Overlay::new();
    frame.add_css_class("avatar");
    frame.set_overflow(gtk::Overflow::Hidden);
    frame.set_valign(gtk::Align::Center);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_size_request(size, size);
    frame.set_child(Some(&spacer));
    let initial = gtk::Label::new(Some("·"));
    initial.set_halign(gtk::Align::Center);
    initial.set_valign(gtk::Align::Center);
    frame.add_overlay(&initial);
    let portrait = gtk::Picture::new();
    portrait.set_content_fit(gtk::ContentFit::Cover);
    portrait.set_visible(false);
    frame.add_overlay(&portrait);

    (frame, initial, portrait)
}

fn badge(initial: &str) -> gtk::Box {
    let badge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    badge.add_css_class("provider-badge");
    badge.set_size_request(40, 40);
    badge.set_hexpand(false);
    badge.set_valign(gtk::Align::Center);
    let letter = gtk::Label::new(Some(initial));
    letter.set_hexpand(true);
    letter.set_halign(gtk::Align::Center);
    badge.append(&letter);

    badge
}

fn provider_row(
    initial: &str,
    name: &str,
    sub: &str,
    trailing: &impl IsA<gtk::Widget>,
) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.append(&badge(initial));
    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);
    text.append(&label(name, "quick-name"));
    text.append(&label(sub, "quick-sub"));
    content.append(&text);
    content.append(trailing);
    let row = gtk::Button::builder().child(&content).build();
    row.add_css_class("quick-row");
    row.set_focus_on_click(false);

    row
}

fn menu_row(leading: &impl IsA<gtk::Widget>, text: &str, note: Option<&str>) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.append(leading);
    let text = label(text, "quick-name");
    text.set_hexpand(true);
    content.append(&text);
    if let Some(note) = note {
        content.append(&label(note, "menu-note"));
    }
    let row = gtk::Button::builder().child(&content).build();
    row.add_css_class("quick-row");
    row.set_focus_on_click(false);

    row
}

fn icon(name: &str, class: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(name);
    button.add_css_class(class);
    button.set_valign(gtk::Align::Center);

    button
}
