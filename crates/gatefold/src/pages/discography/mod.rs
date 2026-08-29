use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

use gatefold_core::{
    images, metadata,
    model::{AlbumRef, ArtistRef, PlaylistRef, ReleaseGroup, TrackInfo},
    player,
};
use relm4::{
    Component, ComponentParts, ComponentSender,
    gtk::{self, prelude::*},
};

use crate::{app::Services, text};

pub const CSS: &str = include_str!("style.css");

const GROUPS: &[ReleaseGroup] = &[
    ReleaseGroup::Albums,
    ReleaseGroup::Singles,
    ReleaseGroup::Compilations,
    ReleaseGroup::AppearsOn,
];

pub struct DiscographyPage {
    services: Option<Arc<Services>>,
    listening: bool,
    requests: tokio::sync::watch::Sender<u64>,
    artist: ArtistRef,
    view: View,
    playing: String,
    active_queue: bool,
    is_playing: bool,
    uris: Vec<String>,
    rows: Vec<(String, gtk::Widget, gtk::Image)>,
    thumbs: Vec<(String, gtk::Picture)>,
    chips: Vec<gtk::Button>,
    query: String,
    next_offset: Option<usize>,
    loading: bool,
    release_count: usize,
    release_grid: Option<gtk::FlowBox>,
    song_list: Option<gtk::Box>,
    owner: gtk::Label,
    title: gtk::Label,
    search: gtk::Entry,
    body: gtk::Box,
    load_more: gtk::Button,
    scroll: gtk::ScrolledWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Releases(ReleaseGroup),
    Songs,
}

impl View {
    pub fn title(self) -> &'static str {
        match self {
            View::Releases(group) => group.title(),
            View::Songs => "Songs",
        }
    }
}

pub enum DiscographyAction {
    Show(Arc<Services>, ArtistRef, View),
    Select(usize),
    Search(String),
    LoadMore,
    PlayTrack(usize),
    OpenAlbum(Box<PlaylistRef>),
    OpenArtist(Box<ArtistRef>),
}

impl std::fmt::Debug for DiscographyAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscographyAction::Show(_, artist, view) => {
                write!(f, "Show({}, {})", artist.name, view.title())
            }
            DiscographyAction::Select(index) => write!(f, "Select({index})"),
            DiscographyAction::Search(query) => write!(f, "Search({query})"),
            DiscographyAction::LoadMore => f.write_str("LoadMore"),
            DiscographyAction::PlayTrack(index) => write!(f, "PlayTrack({index})"),
            DiscographyAction::OpenAlbum(album) => write!(f, "OpenAlbum({})", album.name),
            DiscographyAction::OpenArtist(artist) => write!(f, "OpenArtist({})", artist.name),
        }
    }
}

#[derive(Debug)]
pub enum DiscographyOutput {
    OpenAlbum(Box<PlaylistRef>),
    OpenArtist(Box<ArtistRef>),
}

#[derive(Debug)]
pub enum DiscographyCmd {
    Releases(u64, Vec<AlbumRef>, usize, Option<usize>),
    Songs(u64, Vec<TrackInfo>, usize, Option<usize>),
    Image(u64, String, std::path::PathBuf),
    Playback(player::Event),
    Failed(u64, String),
}

impl Component for DiscographyPage {
    type Init = ();
    type Input = DiscographyAction;
    type Output = DiscographyOutput;
    type CommandOutput = DiscographyCmd;
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.add_css_class("discography-page");
        page.set_hexpand(true);

        page
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let inner = gtk::Box::new(gtk::Orientation::Vertical, 0);
        inner.add_css_class("discography-inner");

        let head = gtk::Box::new(gtk::Orientation::Vertical, 2);
        head.add_css_class("discography-head");
        let owner = gtk::Label::new(None);
        owner.add_css_class("discography-owner");
        owner.set_xalign(0.0);
        head.append(&owner);
        let title = gtk::Label::new(None);
        title.add_css_class("discography-title");
        title.set_xalign(0.0);
        head.append(&title);
        inner.append(&head);

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        controls.add_css_class("discography-controls");
        let filters = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        filters.set_hexpand(true);
        filters.set_valign(gtk::Align::Center);
        let mut chips = Vec::new();
        for (index, view) in GROUPS
            .iter()
            .map(|group| View::Releases(*group))
            .chain(std::iter::once(View::Songs))
            .enumerate()
        {
            let chip = gtk::Button::with_label(view.title());
            chip.add_css_class("filter");
            let select = sender.input_sender().clone();
            chip.connect_clicked(move |_| select.emit(DiscographyAction::Select(index)));
            filters.append(&chip);
            chips.push(chip);
        }
        controls.append(&filters);

        let search = gtk::Entry::new();
        search.add_css_class("search");
        search.add_css_class("discography-search");
        search.set_placeholder_text(Some("Filter"));
        search.set_primary_icon_name(Some("system-search-symbolic"));
        search.set_width_chars(22);
        search.set_valign(gtk::Align::Center);
        search.connect_changed(|search| {
            let empty = search.text().is_empty();
            search.set_secondary_icon_name((!empty).then_some("edit-clear-symbolic"));
        });
        search.connect_icon_press(|search, position| {
            if position == gtk::EntryIconPosition::Secondary {
                search.set_text("");
            }
        });
        let pending = Rc::new(Cell::new(0_u64));
        search.connect_changed({
            let pending = pending.clone();
            let sender = sender.input_sender().clone();
            move |search| {
                let epoch = pending.get().wrapping_add(1);
                pending.set(epoch);
                let text = search.text().to_string();
                if text.trim().is_empty() {
                    sender.emit(DiscographyAction::Search(text));
                    return;
                }
                let pending = pending.clone();
                let sender = sender.clone();
                gtk::glib::timeout_add_local_once(Duration::from_millis(200), move || {
                    if pending.get() == epoch {
                        sender.emit(DiscographyAction::Search(text));
                    }
                });
            }
        });
        search.connect_activate({
            let pending = pending.clone();
            let sender = sender.input_sender().clone();
            move |search| {
                pending.set(pending.get().wrapping_add(1));
                sender.emit(DiscographyAction::Search(search.text().to_string()));
            }
        });
        let escape = gtk::EventControllerKey::new();
        escape.connect_key_pressed({
            let search = search.clone();
            move |_, key, _, _| {
                if key != gtk::gdk::Key::Escape {
                    return gtk::glib::Propagation::Proceed;
                }
                if search.text().is_empty() {
                    if let Some(root) = search.root() {
                        root.set_focus(None::<&gtk::Widget>);
                    }
                } else {
                    search.set_text("");
                }
                gtk::glib::Propagation::Stop
            }
        });
        search.add_controller(escape);
        controls.append(&search);
        inner.append(&controls);

        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        inner.append(&body);

        let load_more = gtk::Button::with_label("Load more");
        load_more.add_css_class("load-more");
        load_more.set_halign(gtk::Align::Center);
        load_more.set_visible(false);
        let load = sender.input_sender().clone();
        load_more.connect_clicked(move |_| load.emit(DiscographyAction::LoadMore));
        inner.append(&load_more);

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(false)
            .vexpand(true)
            .child(&inner)
            .build();
        root.append(&scroll);

        let (requests, _) = tokio::sync::watch::channel(0);
        let model = DiscographyPage {
            services: None,
            listening: false,
            requests,
            artist: ArtistRef {
                uri: String::new(),
                name: String::new(),
            },
            view: View::Releases(ReleaseGroup::Albums),
            playing: String::new(),
            active_queue: false,
            is_playing: false,
            uris: Vec::new(),
            rows: Vec::new(),
            thumbs: Vec::new(),
            chips,
            query: String::new(),
            next_offset: None,
            loading: false,
            release_count: 0,
            release_grid: None,
            song_list: None,
            owner,
            title,
            search,
            body,
            load_more,
            scroll,
        };

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            DiscographyAction::Show(services, artist, view) => {
                self.show(services, artist, view, &sender);
            }
            DiscographyAction::Select(index) => {
                let view = GROUPS
                    .get(index)
                    .map(|group| View::Releases(*group))
                    .unwrap_or(View::Songs);
                let Some(services) = self.services.clone() else {
                    return;
                };
                let artist = self.artist.clone();
                self.show(services, artist, view, &sender);
            }
            DiscographyAction::Search(query) => {
                let query = query.trim().to_owned();
                if query != self.query {
                    self.query = query;
                    self.reload(&sender);
                }
            }
            DiscographyAction::LoadMore => self.load_more(&sender),
            DiscographyAction::PlayTrack(index) => {
                if self.active_queue && self.uris.get(index) == Some(&self.playing) {
                    if let Some(services) = &self.services {
                        services.playback.toggle();
                    }
                } else {
                    self.play(index);
                }
            }
            DiscographyAction::OpenAlbum(album) => {
                let _ = sender.output(DiscographyOutput::OpenAlbum(album));
            }
            DiscographyAction::OpenArtist(artist) => {
                let _ = sender.output(DiscographyOutput::OpenArtist(artist));
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
            DiscographyCmd::Releases(request, releases, total, next) => {
                if request == *self.requests.borrow() {
                    self.page_loaded(next);
                    self.releases(request, &releases, total, next.is_none(), &sender);
                }
            }
            DiscographyCmd::Songs(request, tracks, total, next) => {
                if request == *self.requests.borrow() {
                    self.page_loaded(next);
                    self.songs(request, &tracks, total, next.is_none(), &sender);
                }
            }
            DiscographyCmd::Image(request, key, path) => {
                if request != *self.requests.borrow() {
                    return;
                }
                for (thumb_key, picture) in &self.thumbs {
                    if thumb_key == &key {
                        picture.set_filename(Some(&path));
                    }
                }
            }
            DiscographyCmd::Playback(event) => self.playback(event),
            DiscographyCmd::Failed(request, error) => {
                if request == *self.requests.borrow() {
                    tracing::error!("{error}");
                    self.loading = false;
                    self.load_more.set_label("Try again");
                    self.load_more.set_sensitive(true);
                    self.load_more.set_visible(self.next_offset.is_some());
                    if self.release_count == 0 && self.uris.is_empty() {
                        self.empty("Couldn’t load this view. Please try again.");
                    }
                }
            }
        }
    }

    fn update_view(&self, _: &mut Self::Widgets, _: ComponentSender<Self>) {}
}

impl DiscographyPage {
    fn show(
        &mut self,
        services: Arc<Services>,
        artist: ArtistRef,
        view: View,
        sender: &ComponentSender<Self>,
    ) {
        let artist_changed = self.artist.uri != artist.uri;
        self.artist = artist;
        self.view = view;
        if artist_changed {
            self.query.clear();
            self.search.set_text("");
        }
        self.owner.set_text(&self.artist.name);
        for (index, chip) in self.chips.iter().enumerate() {
            let active = GROUPS
                .get(index)
                .map(|group| View::Releases(*group))
                .unwrap_or(View::Songs)
                == view;
            if active {
                chip.add_css_class("active");
            } else {
                chip.remove_css_class("active");
            }
        }

        if !self.listening {
            self.listening = true;
            let mut events = services.playback.events();
            sender.command(|out, shutdown| {
                shutdown
                    .register(async move {
                        while let Ok(event) = events.recv().await {
                            let _ = out.send(DiscographyCmd::Playback(event));
                        }
                    })
                    .drop_on_shutdown()
            });
        }

        self.services = Some(services);
        self.reload(sender);
    }

    fn reload(&mut self, sender: &ComponentSender<Self>) {
        let request = (*self.requests.borrow()).wrapping_add(1);
        self.requests.send_replace(request);
        self.title.set_text(self.view.title());
        self.scroll.vadjustment().set_value(0.0);
        self.uris.clear();
        self.rows.clear();
        self.thumbs.clear();
        self.release_count = 0;
        self.next_offset = Some(0);
        self.loading = false;
        self.release_grid = None;
        self.song_list = None;
        self.load_more.set_visible(false);
        while let Some(child) = self.body.first_child() {
            self.body.remove(&child);
        }
        self.load_more(sender);
    }

    fn load_more(&mut self, sender: &ComponentSender<Self>) {
        if self.loading {
            return;
        }
        let Some(offset) = self.next_offset else {
            return;
        };
        let Some(services) = self.services.clone() else {
            return;
        };
        if self.release_count == 0 && self.uris.is_empty() {
            while let Some(child) = self.body.first_child() {
                self.body.remove(&child);
            }
        }

        self.loading = true;
        self.load_more.set_label("Loading…");
        self.load_more.set_sensitive(false);
        self.load_more.set_visible(true);

        let request = *self.requests.borrow();
        let session = services.session();
        let uri = self.artist.uri.clone();
        let view = self.view;
        let query = self.query.clone();
        let mut requests = self.requests.subscribe();
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    match view {
                        View::Releases(group) => {
                            tokio::select! {
                                result = metadata::discography_search_page(
                                    &session, &uri, group, &query, Some(offset),
                                ) => {
                                    let message = match result {
                                        Ok((releases, total, next)) => DiscographyCmd::Releases(
                                            request, releases, total, next,
                                        ),
                                        Err(error) => DiscographyCmd::Failed(
                                            request, format!("discography: {error}"),
                                        ),
                                    };
                                    let _ = out.send(message);
                                }
                                _ = requests.changed() => {}
                            }
                        }
                        View::Songs => {
                            tokio::select! {
                                result = metadata::artist_track_search_page(
                                    &session, &uri, &query, Some(offset),
                                ) => {
                                    let message = match result {
                                        Ok((tracks, total, next)) => {
                                            DiscographyCmd::Songs(request, tracks, total, next)
                                        }
                                        Err(error) => DiscographyCmd::Failed(
                                            request, format!("songs: {error}"),
                                        ),
                                    };
                                    let _ = out.send(message);
                                }
                                _ = requests.changed() => {}
                            }
                        }
                    }
                })
                .drop_on_shutdown()
        });
    }

    fn page_loaded(&mut self, next: Option<usize>) {
        self.loading = false;
        self.next_offset = next;
        self.load_more.set_label("Load more");
        self.load_more.set_sensitive(true);
        self.load_more.set_visible(next.is_some());
    }

    fn releases(
        &mut self,
        request: u64,
        releases: &[AlbumRef],
        total: usize,
        done: bool,
        sender: &ComponentSender<Self>,
    ) {
        if releases.is_empty() && self.release_count == 0 && done {
            self.empty_results();
            return;
        }
        self.release_count += releases.len();
        let count = total.max(self.release_count);
        self.title
            .set_text(&format!("{} · {count}", self.view.title()));

        let grid = self
            .release_grid
            .get_or_insert_with(|| {
                let grid = gtk::FlowBox::new();
                grid.set_selection_mode(gtk::SelectionMode::None);
                grid.set_homogeneous(true);
                grid.set_column_spacing(8);
                grid.set_row_spacing(12);
                grid.set_min_children_per_line(2);
                grid.set_max_children_per_line(8);
                grid.set_halign(gtk::Align::Start);
                grid.set_margin_top(8);
                self.body.append(&grid);
                grid
            })
            .clone();
        for release in releases {
            let art = self.art(request, 160, false, release.cover_id.as_deref(), sender);
            let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
            body.append(&art);
            let name = gtk::Label::new(Some(&release.name));
            name.add_css_class("card-name");
            name.set_xalign(0.0);
            name.set_max_width_chars(16);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            body.append(&name);
            let artists: Vec<&str> = release
                .artists
                .iter()
                .map(|artist| artist.name.as_str())
                .collect();
            let sub = if self.view == View::Releases(ReleaseGroup::AppearsOn) && !artists.is_empty()
            {
                format!("{} · {}", release.year, artists.join(", "))
            } else {
                release.year.to_string()
            };
            let sub = gtk::Label::new(Some(&sub));
            sub.add_css_class("card-sub");
            sub.set_xalign(0.0);
            sub.set_max_width_chars(18);
            sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
            body.append(&sub);

            let card = gtk::Button::builder().child(&body).build();
            card.add_css_class("card");
            let entry = PlaylistRef {
                uri: release.uri.clone(),
                name: release.name.clone(),
                owner: release
                    .artists
                    .first()
                    .map(|artist| artist.name.clone())
                    .unwrap_or_else(|| self.artist.name.clone()),
                length: 0,
                picture: release.cover_id.clone(),
            };
            let open = sender.input_sender().clone();
            card.connect_clicked(move |_| {
                open.emit(DiscographyAction::OpenAlbum(Box::new(entry.clone())));
            });
            grid.append(&card);
        }
    }

    fn songs(
        &mut self,
        request: u64,
        tracks: &[TrackInfo],
        total: usize,
        done: bool,
        sender: &ComponentSender<Self>,
    ) {
        if tracks.is_empty() && self.uris.is_empty() && done {
            self.empty_results();
            return;
        }

        let list = self
            .song_list
            .get_or_insert_with(|| {
                let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
                list.set_margin_top(8);
                self.body.append(&list);
                list
            })
            .clone();
        for track in tracks {
            if self.uris.contains(&track.uri) {
                continue;
            }
            let index = self.uris.len();
            self.uris.push(track.uri.clone());
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);

            let leading = gtk::Overlay::new();
            leading.add_css_class("track-leading");
            leading.set_size_request(24, -1);
            let number = gtk::Label::new(Some(&format!("{:>3}", index + 1)));
            number.add_css_class("track-index");
            number.set_xalign(1.0);
            leading.set_child(Some(&number));
            let track_play = gtk::Image::from_icon_name("media-playback-start-symbolic");
            track_play.add_css_class("track-play");
            track_play.set_halign(gtk::Align::Center);
            track_play.set_valign(gtk::Align::Center);
            leading.add_overlay(&track_play);
            let equalizer = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            equalizer.add_css_class("track-equalizer");
            equalizer.set_halign(gtk::Align::Center);
            equalizer.set_valign(gtk::Align::Center);
            for _ in 0..3 {
                let bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
                bar.add_css_class("track-equalizer-bar");
                bar.set_valign(gtk::Align::End);
                equalizer.append(&bar);
            }
            leading.add_overlay(&equalizer);
            row.append(&leading);

            let tile = self.art(request, 40, false, track.cover_id.as_deref(), sender);
            tile.add_css_class("track-art");
            tile.set_valign(gtk::Align::Center);
            row.append(&tile);

            let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
            text.set_valign(gtk::Align::Center);
            text.set_hexpand(true);
            let name = gtk::Label::new(Some(&track.name));
            name.add_css_class("track-name");
            name.set_xalign(0.0);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(&name);
            let open = sender.input_sender().clone();
            let artists = crate::artists::label(&track.artists, move |artist| {
                open.emit(DiscographyAction::OpenArtist(Box::new(artist)));
            });
            text.append(&artists);
            row.append(&text);

            if let Some(plays) = track.plays {
                let plays = gtk::Label::new(Some(&text::thousands(plays)));
                plays.add_css_class("track-plays");
                plays.set_xalign(1.0);
                plays.set_size_request(120, -1);
                row.append(&plays);
            }

            let time = gtk::Label::new(Some(&clock(track.duration_ms)));
            time.add_css_class("track-time");
            row.append(&time);

            let button = gtk::Button::builder().child(&row).build();
            button.add_css_class("track");
            let play = sender.input_sender().clone();
            button.connect_clicked(move |_| play.emit(DiscographyAction::PlayTrack(index)));
            self.rows
                .push((track.uri.clone(), button.clone().upcast(), track_play));
            list.append(&button);
        }
        self.title
            .set_text(&format!("Songs · {}", total.max(self.uris.len())));
        self.sync_playback();
    }

    fn empty_results(&self) {
        if self.query.is_empty() {
            self.empty("Nothing here yet.");
        } else {
            self.empty(&format!("No matches for “{}”.", self.query));
        }
    }

    fn empty(&self, message: &str) {
        let label = gtk::Label::new(Some(message));
        label.add_css_class("discography-empty");
        label.set_xalign(0.0);
        self.body.append(&label);
    }

    fn art(
        &mut self,
        request: u64,
        size: i32,
        round: bool,
        picture: Option<&str>,
        sender: &ComponentSender<Self>,
    ) -> gtk::Box {
        let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tile.add_css_class("card-art");
        if round {
            tile.add_css_class("round");
        }
        tile.set_size_request(size, size);
        tile.set_hexpand(false);
        tile.set_halign(gtk::Align::Start);
        tile.set_overflow(gtk::Overflow::Hidden);

        let Some(picture) = picture else {
            let icon = gtk::Image::from_icon_name("audio-x-generic-symbolic");
            icon.set_pixel_size(if size > 64 { 28 } else { 16 });
            icon.set_halign(gtk::Align::Center);
            icon.set_hexpand(true);
            tile.append(&icon);
            return tile;
        };

        let display = gtk::Picture::new();
        display.set_content_fit(gtk::ContentFit::Cover);
        let frame = gtk::Overlay::new();
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_size_request(size, size);
        frame.set_child(Some(&spacer));
        frame.add_overlay(&display);
        tile.append(&frame);

        if let Some(path) = images::cached(picture) {
            display.set_filename(Some(&path));
            return tile;
        }
        self.thumbs.push((picture.to_owned(), display));
        if let Some(services) = self.services.clone() {
            let picture = picture.to_owned();
            let mut requests = self.requests.subscribe();
            sender.command(move |out, shutdown| {
                shutdown
                    .register(async move {
                        let session = services.session();
                        tokio::select! {
                            result = images::fetch(&session, &picture) => {
                                if let Ok(path) = result {
                                    let _ = out.send(DiscographyCmd::Image(request, picture, path));
                                }
                            }
                            _ = requests.changed() => {}
                        }
                    })
                    .drop_on_shutdown()
            });
        }

        tile
    }

    fn play(&mut self, index: usize) {
        let Some(services) = &self.services else {
            return;
        };
        if self.uris.is_empty() {
            return;
        }
        services.playback.play_queue(self.uris.clone(), index);
        self.active_queue = true;
        self.is_playing = true;
        if let Some(uri) = self.uris.get(index) {
            self.playing.clone_from(uri);
        }
        self.refresh_rows();
    }

    fn playback(&mut self, event: player::Event) {
        match event {
            player::Event::Loading { uri } | player::Event::TrackChanged { uri, .. } => {
                self.playing = uri;
            }
            player::Event::Playing { uri, .. } => {
                self.playing = uri;
                self.is_playing = true;
            }
            player::Event::Paused { uri, .. } => {
                self.playing = uri;
                self.is_playing = false;
            }
            player::Event::QueueChanged { .. } => self.sync_playback(),
            player::Event::Stopped => {
                self.active_queue = false;
                self.is_playing = false;
            }
            _ => return,
        }

        self.refresh_rows();
    }

    fn sync_playback(&mut self) {
        let Some(services) = &self.services else {
            return;
        };
        let (queue, index) = services.playback.queue();
        self.active_queue = same_queue(&self.uris, &queue);
        self.is_playing = services.playback.is_playing();
        if let Some(uri) = queue.get(index) {
            self.playing.clone_from(uri);
        }
        self.refresh_rows();
    }

    fn refresh_rows(&self) {
        for (uri, row, play) in &self.rows {
            if *uri == self.playing {
                row.add_css_class("playing");
            } else {
                row.remove_css_class("playing");
            }
            play.set_icon_name(Some(
                if self.active_queue && self.is_playing && *uri == self.playing {
                    "media-playback-pause-symbolic"
                } else {
                    "media-playback-start-symbolic"
                },
            ));
            if self.active_queue && self.is_playing && *uri == self.playing {
                row.add_css_class("playing-active");
            } else {
                row.remove_css_class("playing-active");
            }
        }
    }
}

fn same_queue(left: &[String], right: &[String]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut left: Vec<&str> = left.iter().map(String::as_str).collect();
    let mut right: Vec<&str> = right.iter().map(String::as_str).collect();
    left.sort_unstable();
    right.sort_unstable();
    left == right
}

fn clock(ms: u32) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
