use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use gatefold_core::{
    images, metadata,
    model::{ArtistRef, PlaylistRef, SearchOptions, SearchResults, SearchType},
};
use relm4::{Component, ComponentParts, ComponentSender, adw, adw::prelude::*, gtk};

use crate::app::Services;

pub const CSS: &str = include_str!("style.css");

const MAX_SEARCH: i32 = 560;

pub struct Topbar {
    can_back: bool,
    can_forward: bool,
    search: gtk::Entry,
    services: Option<Arc<Services>>,
    request: u64,
    quick: gtk::Popover,
    rows: gtk::Box,
    thumbs: Vec<(String, gtk::Picture)>,
}

#[derive(Clone)]
pub enum TopbarAction {
    History { back: bool, forward: bool },
    ToggleRack,
    Back,
    Forward,
    FocusSearch,
    SetServices(Arc<Services>),
    Quick(String),
    Submit(String),
    Play(String),
    OpenPlaylist(Box<PlaylistRef>),
    OpenArtist(Box<ArtistRef>, Option<String>),
    Dismiss,
}

impl std::fmt::Debug for TopbarAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TopbarAction::History { back, forward } => write!(f, "History({back}, {forward})"),
            TopbarAction::ToggleRack => write!(f, "ToggleRack"),
            TopbarAction::Back => write!(f, "Back"),
            TopbarAction::Forward => write!(f, "Forward"),
            TopbarAction::FocusSearch => write!(f, "FocusSearch"),
            TopbarAction::SetServices(_) => write!(f, "SetServices"),
            TopbarAction::Quick(query) => write!(f, "Quick({query})"),
            TopbarAction::Submit(query) => write!(f, "Submit({query})"),
            TopbarAction::Play(uri) => write!(f, "Play({uri})"),
            TopbarAction::OpenPlaylist(playlist) => write!(f, "OpenPlaylist({})", playlist.name),
            TopbarAction::OpenArtist(artist, _) => write!(f, "OpenArtist({})", artist.name),
            TopbarAction::Dismiss => write!(f, "Dismiss"),
        }
    }
}

#[derive(Debug)]
pub enum TopbarOutput {
    ToggleRack,
    Back,
    Forward,
    Search(String),
    OpenPlaylist(Box<PlaylistRef>),
    OpenArtist(Box<ArtistRef>, Option<String>),
}

struct QuickRow<'a> {
    request: u64,
    picture: Option<&'a str>,
    round: bool,
    name: &'a str,
    sub: &'a str,
    action: TopbarAction,
}

#[derive(Debug)]
pub enum TopbarCmd {
    Results(u64, Box<SearchResults>),
    Image(u64, String, std::path::PathBuf),
    Failed(String),
}

#[relm4::component(pub)]
impl Component for Topbar {
    type Init = ();
    type Input = TopbarAction;
    type Output = TopbarOutput;
    type CommandOutput = TopbarCmd;

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
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let search = gtk::Entry::new();
        let quick = gtk::Popover::new();
        quick.set_parent(&search);
        quick.set_position(gtk::PositionType::Bottom);
        quick.set_autohide(false);
        quick.set_has_arrow(false);
        quick.set_offset(0, 6);
        quick.remove_css_class("background");
        quick.add_css_class("quick-menu");
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 2);
        rows.set_width_request(380);
        quick.set_child(Some(&rows));

        let model = Topbar {
            can_back: false,
            can_forward: false,
            search: search.clone(),
            services: None,
            request: 0,
            quick,
            rows,
            thumbs: Vec::new(),
        };
        let widgets = view_output!();

        search.connect_changed(|search| {
            let empty = search.text().is_empty();
            search.set_secondary_icon_name((!empty).then_some("edit-clear-symbolic"));
        });

        let pending = Rc::new(Cell::new(0_u64));
        search.connect_changed({
            let pending = pending.clone();
            let sender = sender.clone();
            move |search| {
                let epoch = pending.get().wrapping_add(1);
                pending.set(epoch);
                let text = search.text().to_string();
                if text.trim().is_empty() {
                    sender.input(TopbarAction::Dismiss);
                    return;
                }
                let pending = pending.clone();
                let sender = sender.clone();
                gtk::glib::timeout_add_local_once(
                    std::time::Duration::from_millis(350),
                    move || {
                        if pending.get() == epoch {
                            sender.input(TopbarAction::Quick(text));
                        }
                    },
                );
            }
        });
        search.connect_activate({
            let pending = pending.clone();
            let sender = sender.clone();
            move |search| {
                pending.set(pending.get().wrapping_add(1));
                let text = search.text().to_string();
                if !text.trim().is_empty() {
                    sender.input(TopbarAction::Submit(text));
                }
            }
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
            let sender = sender.clone();
            move |_| {
                sender.input(TopbarAction::Dismiss);
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
            TopbarAction::SetServices(services) => self.services = Some(services),
            TopbarAction::Quick(query) => self.quick_search(query, &sender),
            TopbarAction::Submit(query) => {
                self.quick.popdown();
                let _ = sender.output(TopbarOutput::Search(query));
            }
            TopbarAction::Play(uri) => {
                self.quick.popdown();
                if let Some(services) = &self.services {
                    services.playback.play_queue(vec![uri], 0);
                }
            }
            TopbarAction::OpenPlaylist(playlist) => {
                self.quick.popdown();
                let _ = sender.output(TopbarOutput::OpenPlaylist(playlist));
            }
            TopbarAction::OpenArtist(artist, picture) => {
                self.quick.popdown();
                let _ = sender.output(TopbarOutput::OpenArtist(artist, picture));
            }
            TopbarAction::Dismiss => self.quick.popdown(),
        }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            TopbarCmd::Results(request, results) => {
                if request == self.request {
                    self.present(request, *results, &sender);
                }
            }
            TopbarCmd::Image(request, key, path) => {
                if request != self.request {
                    return;
                }
                for (thumb_key, picture) in &self.thumbs {
                    if thumb_key == &key {
                        picture.set_filename(Some(&path));
                    }
                }
            }
            TopbarCmd::Failed(error) => tracing::error!("{error}"),
        }
    }
}

impl Topbar {
    fn quick_search(&mut self, query: String, sender: &ComponentSender<Self>) {
        let Some(services) = self.services.clone() else {
            return;
        };
        self.request = self.request.wrapping_add(1);
        let request = self.request;
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    let options = SearchOptions {
                        types: vec![
                            SearchType::Track,
                            SearchType::Album,
                            SearchType::Artist,
                            SearchType::Playlist,
                        ],
                        ..SearchOptions::default()
                    };
                    let message =
                        match metadata::search(&services.session(), &query, &options).await {
                            Ok(results) => TopbarCmd::Results(request, Box::new(results)),
                            Err(error) => TopbarCmd::Failed(format!("quick search: {error}")),
                        };
                    let _ = out.send(message);
                })
                .drop_on_shutdown()
        });
    }

    fn present(&mut self, request: u64, results: SearchResults, sender: &ComponentSender<Self>) {
        let width = self.search.width();
        if width > 0 {
            self.rows.set_width_request(width - 16);
        }
        self.thumbs.clear();
        while let Some(child) = self.rows.first_child() {
            self.rows.remove(&child);
        }

        let tracks = results.tracks.map(|page| page.items).unwrap_or_default();
        let albums = results.albums.map(|page| page.items).unwrap_or_default();
        let artists = results.artists.map(|page| page.items).unwrap_or_default();
        let playlists = results.playlists.map(|page| page.items).unwrap_or_default();

        for artist in artists.iter().take(2) {
            let entry = ArtistRef {
                uri: artist.uri.clone(),
                name: artist.name.clone(),
            };
            let row = self.row(
                QuickRow {
                    request,
                    picture: artist.portrait.as_deref(),
                    round: true,
                    name: &artist.name,
                    sub: "Artist",
                    action: TopbarAction::OpenArtist(Box::new(entry), artist.portrait.clone()),
                },
                sender,
            );
            self.rows.append(&row);
        }
        for track in tracks.iter().take(3) {
            let artist = track
                .artists
                .first()
                .map(|artist| artist.name.as_str())
                .unwrap_or_default();
            let sub = format!("Song · {artist}");
            let row = self.row(
                QuickRow {
                    request,
                    picture: track.album.cover.as_deref(),
                    round: false,
                    name: &track.name,
                    sub: &sub,
                    action: TopbarAction::Play(track.uri.clone()),
                },
                sender,
            );
            self.rows.append(&row);
        }
        for album in albums.iter().take(2) {
            let artist = album
                .artists
                .first()
                .map(|artist| artist.name.as_str())
                .unwrap_or_default();
            let entry = PlaylistRef {
                uri: album.uri.clone(),
                name: album.name.clone(),
                owner: artist.to_owned(),
                length: album.total_tracks as usize,
                picture: album.cover.clone(),
            };
            let sub = format!("Album · {artist}");
            let row = self.row(
                QuickRow {
                    request,
                    picture: album.cover.as_deref(),
                    round: false,
                    name: &album.name,
                    sub: &sub,
                    action: TopbarAction::OpenPlaylist(Box::new(entry)),
                },
                sender,
            );
            self.rows.append(&row);
        }
        for playlist in playlists.iter().take(1) {
            let entry = PlaylistRef {
                uri: playlist.uri.clone(),
                name: playlist.name.clone(),
                owner: playlist.owner.name.clone(),
                length: playlist.total_tracks as usize,
                picture: playlist.picture.clone(),
            };
            let sub = format!("Playlist · By {}", playlist.owner.name);
            let row = self.row(
                QuickRow {
                    request,
                    picture: playlist.picture.as_deref(),
                    round: false,
                    name: &playlist.name,
                    sub: &sub,
                    action: TopbarAction::OpenPlaylist(Box::new(entry)),
                },
                sender,
            );
            self.rows.append(&row);
        }

        let focused = self
            .search
            .state_flags()
            .contains(gtk::StateFlags::FOCUS_WITHIN);
        if self.rows.first_child().is_none() || !focused {
            self.quick.popdown();
        } else {
            self.quick.popup();
        }
    }

    fn row(&mut self, row: QuickRow<'_>, sender: &ComponentSender<Self>) -> gtk::Button {
        let QuickRow {
            request,
            picture,
            round,
            name,
            sub,
            action,
        } = row;
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);

        let art = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        art.add_css_class("quick-art");
        if round {
            art.add_css_class("round");
        }
        art.set_hexpand(false);
        art.set_valign(gtk::Align::Center);
        art.set_overflow(gtk::Overflow::Hidden);
        match picture {
            Some(picture) => {
                let display = gtk::Picture::new();
                display.set_content_fit(gtk::ContentFit::Cover);
                let frame = gtk::Overlay::new();
                let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                spacer.set_size_request(36, 36);
                frame.set_child(Some(&spacer));
                frame.add_overlay(&display);
                art.append(&frame);
                if let Some(path) = images::cached(picture) {
                    display.set_filename(Some(&path));
                } else {
                    self.thumbs.push((picture.to_owned(), display));
                    if let Some(services) = self.services.clone() {
                        let picture = picture.to_owned();
                        sender.command(move |out, shutdown| {
                            shutdown
                                .register(async move {
                                    if let Ok(path) =
                                        images::fetch(&services.session(), &picture).await
                                    {
                                        let _ = out.send(TopbarCmd::Image(request, picture, path));
                                    }
                                })
                                .drop_on_shutdown()
                        });
                    }
                }
            }
            None => {
                art.set_size_request(36, 36);
                let icon = gtk::Image::from_icon_name("audio-x-generic-symbolic");
                icon.set_halign(gtk::Align::Center);
                icon.set_hexpand(true);
                art.append(&icon);
            }
        }
        content.append(&art);

        let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
        text.set_valign(gtk::Align::Center);
        text.set_hexpand(true);
        let name_label = gtk::Label::new(Some(name));
        name_label.add_css_class("quick-name");
        name_label.set_xalign(0.0);
        name_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&name_label);
        let sub_label = gtk::Label::new(Some(sub));
        sub_label.add_css_class("quick-sub");
        sub_label.set_xalign(0.0);
        sub_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&sub_label);
        content.append(&text);

        let button = gtk::Button::builder().child(&content).build();
        button.add_css_class("quick-row");
        button.set_focus_on_click(false);
        let input = sender.input_sender().clone();
        button.connect_clicked(move |_| input.emit(action.clone()));

        button
    }
}
