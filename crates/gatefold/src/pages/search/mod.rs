use std::sync::Arc;

use gatefold_core::{
    images, metadata,
    model::{
        ArtistRef, PlaylistRef, SearchAlbum, SearchArtist, SearchOptions, SearchPlaylist,
        SearchResults, SearchTrack, SearchType,
    },
    player,
};
use relm4::{
    Component, ComponentParts, ComponentSender,
    gtk::{self, prelude::*},
};

use crate::app::Services;

pub const CSS: &str = include_str!("style.css");

pub struct SearchPage {
    services: Option<Arc<Services>>,
    listening: bool,
    requests: tokio::sync::watch::Sender<u64>,
    query: String,
    playing: String,
    active_queue: bool,
    is_playing: bool,
    track_uris: Vec<String>,
    rows: Vec<(String, gtk::Widget, gtk::Image)>,
    thumbs: Vec<(String, gtk::Picture)>,
    heading: gtk::Label,
    sections: gtk::Box,
}

pub enum SearchAction {
    Show(Arc<Services>),
    Query(String),
    PlayTrack(usize),
    OpenPlaylist(Box<PlaylistRef>),
    OpenArtist(Box<ArtistRef>, Option<String>),
}

impl std::fmt::Debug for SearchAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchAction::Show(_) => write!(f, "Show"),
            SearchAction::Query(query) => write!(f, "Query({query})"),
            SearchAction::PlayTrack(index) => write!(f, "PlayTrack({index})"),
            SearchAction::OpenPlaylist(playlist) => write!(f, "OpenPlaylist({})", playlist.name),
            SearchAction::OpenArtist(artist, _) => write!(f, "OpenArtist({})", artist.name),
        }
    }
}

#[derive(Debug)]
pub enum SearchOutput {
    OpenPlaylist(Box<PlaylistRef>),
    OpenArtist(Box<ArtistRef>, Option<String>),
}

#[derive(Debug)]
pub enum SearchCmd {
    Results(u64, Box<SearchResults>),
    Image(u64, String, std::path::PathBuf),
    Playback(player::Event),
    Failed(String),
}

impl Component for SearchPage {
    type Init = ();
    type Input = SearchAction;
    type Output = SearchOutput;
    type CommandOutput = SearchCmd;
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.add_css_class("search-page");
        page.set_hexpand(true);

        page
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let inner = gtk::Box::new(gtk::Orientation::Vertical, 0);
        inner.add_css_class("search-inner");

        let heading = gtk::Label::new(None);
        heading.add_css_class("search-heading");
        heading.set_xalign(0.0);
        inner.append(&heading);

        let sections = gtk::Box::new(gtk::Orientation::Vertical, 0);
        inner.append(&sections);

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(false)
            .vexpand(true)
            .child(&inner)
            .build();
        root.append(&scroll);

        let (requests, _) = tokio::sync::watch::channel(0);
        let model = SearchPage {
            services: None,
            listening: false,
            requests,
            query: String::new(),
            playing: String::new(),
            active_queue: false,
            is_playing: false,
            track_uris: Vec::new(),
            rows: Vec::new(),
            thumbs: Vec::new(),
            heading,
            sections,
        };

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            SearchAction::Show(services) => {
                if !self.listening {
                    self.listening = true;
                    let mut events = services.playback.events();
                    sender.command(|out, shutdown| {
                        shutdown
                            .register(async move {
                                while let Ok(event) = events.recv().await {
                                    let _ = out.send(SearchCmd::Playback(event));
                                }
                            })
                            .drop_on_shutdown()
                    });
                }
                self.services = Some(services);
            }
            SearchAction::Query(query) => self.run_query(query, &sender),
            SearchAction::PlayTrack(index) => self.play_from(index),
            SearchAction::OpenPlaylist(playlist) => {
                let _ = sender.output(SearchOutput::OpenPlaylist(playlist));
            }
            SearchAction::OpenArtist(artist, picture) => {
                let _ = sender.output(SearchOutput::OpenArtist(artist, picture));
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
            SearchCmd::Results(request, results) => {
                if request == *self.requests.borrow() {
                    self.render(request, *results, &sender);
                }
            }
            SearchCmd::Image(request, key, path) => {
                if request != *self.requests.borrow() {
                    return;
                }
                for (thumb_key, picture) in &self.thumbs {
                    if thumb_key == &key {
                        picture.set_filename(Some(&path));
                    }
                }
            }
            SearchCmd::Playback(event) => self.playback(event),
            SearchCmd::Failed(error) => tracing::error!("{error}"),
        }
    }

    fn update_view(&self, _: &mut Self::Widgets, _: ComponentSender<Self>) {}
}

impl SearchPage {
    fn run_query(&mut self, query: String, sender: &ComponentSender<Self>) {
        if query == self.query {
            return;
        }
        self.query = query.clone();
        self.heading
            .set_text(&format!("Results for \u{201c}{query}\u{201d}"));

        let request = (*self.requests.borrow()).wrapping_add(1);
        self.requests.send_replace(request);

        let Some(services) = self.services.clone() else {
            return;
        };
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
                        limit: 10,
                        ..SearchOptions::default()
                    };
                    let message = match metadata::search(&services.session, &query, &options).await
                    {
                        Ok(results) => SearchCmd::Results(request, Box::new(results)),
                        Err(error) => SearchCmd::Failed(format!("search: {error}")),
                    };
                    let _ = out.send(message);
                })
                .drop_on_shutdown()
        });
    }

    fn render(&mut self, request: u64, results: SearchResults, sender: &ComponentSender<Self>) {
        self.thumbs.clear();
        self.rows.clear();
        self.track_uris.clear();
        while let Some(child) = self.sections.first_child() {
            self.sections.remove(&child);
        }

        let query = self.query.to_lowercase();
        let relevant = |index: usize, names: &[&str]| {
            index < 5 || names.iter().any(|name| score(name, &query) > 0)
        };
        let named = |artists: &[gatefold_core::model::ArtistRef]| {
            artists
                .iter()
                .map(|artist| artist.name.clone())
                .collect::<Vec<_>>()
        };

        let tracks: Vec<_> = results
            .tracks
            .map(|page| page.items)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .filter(|(index, track)| {
                let artists = named(&track.artists);
                let mut names = vec![track.name.as_str()];
                names.extend(artists.iter().map(String::as_str));
                relevant(*index, &names)
            })
            .map(|(_, track)| track)
            .collect();
        let albums: Vec<_> = results
            .albums
            .map(|page| page.items)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .filter(|(index, album)| {
                let artists = named(&album.artists);
                let mut names = vec![album.name.as_str()];
                names.extend(artists.iter().map(String::as_str));
                relevant(*index, &names)
            })
            .map(|(_, album)| album)
            .collect();
        let artists: Vec<_> = results
            .artists
            .map(|page| page.items)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .filter(|(index, artist)| relevant(*index, &[artist.name.as_str()]))
            .map(|(_, artist)| artist)
            .collect();
        let playlists: Vec<_> = results
            .playlists
            .map(|page| page.items)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .filter(|(index, playlist)| {
                relevant(
                    *index,
                    &[playlist.name.as_str(), playlist.owner.name.as_str()],
                )
            })
            .map(|(_, playlist)| playlist)
            .collect();

        if tracks.is_empty() && albums.is_empty() && artists.is_empty() && playlists.is_empty() {
            let empty = gtk::Label::new(Some("Nothing found."));
            empty.add_css_class("search-empty");
            empty.set_xalign(0.0);
            self.sections.append(&empty);
            return;
        }

        self.track_uris = tracks.iter().map(|track| track.uri.clone()).collect();

        let hero = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        if let Some(card) = self.top_pick(request, &tracks, &artists, &albums, &playlists, sender) {
            let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
            column.append(&section("Top result"));
            column.append(&card);
            hero.append(&column);
        }
        if !tracks.is_empty() {
            let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
            column.set_hexpand(true);
            column.append(&section("Songs"));
            let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
            column.append(&list);
            hero.append(&column);
            for (index, track) in tracks.iter().take(6).enumerate() {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);

                let leading = gtk::Overlay::new();
                leading.add_css_class("track-leading");
                leading.set_size_request(20, -1);
                let number = gtk::Label::new(Some(&format!("{:>2}", index + 1)));
                number.add_css_class("track-index");
                number.set_xalign(1.0);
                leading.set_child(Some(&number));
                let track_play = gtk::Image::from_icon_name(
                    if self.active_queue && self.is_playing && track.uri == self.playing {
                        "media-playback-pause-symbolic"
                    } else {
                        "media-playback-start-symbolic"
                    },
                );
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

                let tile = self.art(request, 40, false, track.album.cover.as_deref(), sender);
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
                    open.emit(SearchAction::OpenArtist(Box::new(artist), None));
                });
                text.append(&artists);
                row.append(&text);

                let time = gtk::Label::new(Some(&clock(track.duration_ms)));
                time.add_css_class("track-time");
                row.append(&time);

                let button = gtk::Button::builder().child(&row).build();
                button.add_css_class("track");
                if track.uri == self.playing {
                    button.add_css_class("playing");
                }
                let play = sender.input_sender().clone();
                button.connect_clicked(move |_| play.emit(SearchAction::PlayTrack(index)));
                self.rows
                    .push((track.uri.clone(), button.clone().upcast(), track_play));
                list.append(&button);
            }
        }
        self.sections.append(&hero);

        if !artists.is_empty() {
            self.sections.append(&section("Artists"));
            let shelf = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            for artist in &artists {
                let card = self.card(
                    request,
                    artist.portrait.as_deref(),
                    true,
                    &artist.name,
                    "Artist",
                    sender,
                );
                let open = sender.input_sender().clone();
                let entry = ArtistRef {
                    uri: artist.uri.clone(),
                    name: artist.name.clone(),
                };
                let portrait = artist.portrait.clone();
                card.connect_clicked(move |_| {
                    open.emit(SearchAction::OpenArtist(
                        Box::new(entry.clone()),
                        portrait.clone(),
                    ));
                });
                shelf.append(&card);
            }
            self.sections.append(&scroller(&shelf));
        }

        if !albums.is_empty() {
            self.sections.append(&section("Albums"));
            let shelf = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            for album in &albums {
                let year = album.release_date.get(..4).unwrap_or_default();
                let artist = album
                    .artists
                    .first()
                    .map(|artist| artist.name.as_str())
                    .unwrap_or_default();
                let card = self.card(
                    request,
                    album.cover.as_deref(),
                    false,
                    &album.name,
                    &format!("{artist} · {year}"),
                    sender,
                );
                let open = sender.input_sender().clone();
                let entry = PlaylistRef {
                    uri: album.uri.clone(),
                    name: album.name.clone(),
                    owner: artist.to_owned(),
                    length: album.total_tracks as usize,
                    picture: album.cover.clone(),
                };
                card.connect_clicked(move |_| {
                    open.emit(SearchAction::OpenPlaylist(Box::new(entry.clone())));
                });
                shelf.append(&card);
            }
            self.sections.append(&scroller(&shelf));
        }

        if !playlists.is_empty() {
            self.sections.append(&section("Playlists"));
            let shelf = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            for playlist in &playlists {
                let card = self.card(
                    request,
                    playlist.picture.as_deref(),
                    false,
                    &playlist.name,
                    &format!("By {}", playlist.owner.name),
                    sender,
                );
                let open = sender.input_sender().clone();
                let entry = PlaylistRef {
                    uri: playlist.uri.clone(),
                    name: playlist.name.clone(),
                    owner: playlist.owner.name.clone(),
                    length: playlist.total_tracks as usize,
                    picture: playlist.picture.clone(),
                };
                card.connect_clicked(move |_| {
                    open.emit(SearchAction::OpenPlaylist(Box::new(entry.clone())));
                });
                shelf.append(&card);
            }
            self.sections.append(&scroller(&shelf));
        }

        self.sync_playback();
    }

    fn top_pick(
        &mut self,
        request: u64,
        tracks: &[SearchTrack],
        artists: &[SearchArtist],
        albums: &[SearchAlbum],
        playlists: &[SearchPlaylist],
        sender: &ComponentSender<Self>,
    ) -> Option<gtk::Button> {
        let query = self.query.to_lowercase();
        let mut best: Option<(u64, Pick)> = None;
        let mut consider = |points: u64, pick: Pick| {
            if best.as_ref().is_none_or(|(top, _)| points > *top) {
                best = Some((points, pick));
            }
        };
        for artist in artists.iter().take(3) {
            let points = score(&artist.name, &query) + artist.popularity as u64 / 10 + 3;
            consider(points, Pick::Artist(artist.clone()));
        }
        for (index, track) in tracks.iter().enumerate().take(3) {
            let points = score(&track.name, &query) + track.popularity as u64 / 10 + 2;
            consider(points, Pick::Track(index, track.clone()));
        }
        if let Some(album) = albums.first() {
            consider(score(&album.name, &query) + 1, Pick::Album(album.clone()));
        }
        if let Some(playlist) = playlists.first() {
            consider(
                score(&playlist.name, &query),
                Pick::Playlist(playlist.clone()),
            );
        }
        let (_, pick) = best?;

        let (picture, round, name, sub) = match &pick {
            Pick::Artist(artist) => (
                artist.portrait.clone(),
                true,
                artist.name.clone(),
                "Artist".to_owned(),
            ),
            Pick::Track(_, track) => {
                let artist = track
                    .artists
                    .first()
                    .map(|artist| artist.name.as_str())
                    .unwrap_or_default();
                (
                    track.album.cover.clone(),
                    false,
                    track.name.clone(),
                    format!("Song · {artist}"),
                )
            }
            Pick::Album(album) => {
                let artist = album
                    .artists
                    .first()
                    .map(|artist| artist.name.as_str())
                    .unwrap_or_default();
                (
                    album.cover.clone(),
                    false,
                    album.name.clone(),
                    format!("Album · {artist}"),
                )
            }
            Pick::Playlist(playlist) => (
                playlist.picture.clone(),
                false,
                playlist.name.clone(),
                format!("Playlist · By {}", playlist.owner.name),
            ),
        };

        let art = self.art(request, 128, round, picture.as_deref(), sender);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.append(&art);
        let name_label = gtk::Label::new(Some(&name));
        name_label.add_css_class("top-name");
        name_label.set_xalign(0.0);
        name_label.set_halign(gtk::Align::Start);
        name_label.set_max_width_chars(14);
        name_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        body.append(&name_label);
        let pill = gtk::Label::new(Some(&sub));
        pill.add_css_class("type-pill");
        pill.set_halign(gtk::Align::Start);
        pill.set_max_width_chars(18);
        pill.set_ellipsize(gtk::pango::EllipsizeMode::End);
        body.append(&pill);

        let card = gtk::Button::builder().child(&body).build();
        card.add_css_class("card");
        card.add_css_class("top-card");
        card.set_valign(gtk::Align::Start);
        match pick {
            Pick::Track(index, _) => {
                let play = sender.input_sender().clone();
                card.connect_clicked(move |_| play.emit(SearchAction::PlayTrack(index)));
            }
            Pick::Playlist(playlist) => {
                let open = sender.input_sender().clone();
                let entry = PlaylistRef {
                    uri: playlist.uri.clone(),
                    name: playlist.name.clone(),
                    owner: playlist.owner.name.clone(),
                    length: playlist.total_tracks as usize,
                    picture: playlist.picture.clone(),
                };
                card.connect_clicked(move |_| {
                    open.emit(SearchAction::OpenPlaylist(Box::new(entry.clone())));
                });
            }
            Pick::Album(album) => {
                let open = sender.input_sender().clone();
                let entry = PlaylistRef {
                    uri: album.uri.clone(),
                    name: album.name.clone(),
                    owner: album
                        .artists
                        .first()
                        .map(|artist| artist.name.clone())
                        .unwrap_or_default(),
                    length: album.total_tracks as usize,
                    picture: album.cover.clone(),
                };
                card.connect_clicked(move |_| {
                    open.emit(SearchAction::OpenPlaylist(Box::new(entry.clone())));
                });
            }
            Pick::Artist(artist) => {
                let open = sender.input_sender().clone();
                let entry = ArtistRef {
                    uri: artist.uri.clone(),
                    name: artist.name.clone(),
                };
                let portrait = artist.portrait.clone();
                card.connect_clicked(move |_| {
                    open.emit(SearchAction::OpenArtist(
                        Box::new(entry.clone()),
                        portrait.clone(),
                    ));
                });
            }
        }

        Some(card)
    }

    fn play_from(&mut self, index: usize) {
        let Some(services) = &self.services else {
            return;
        };
        if self.track_uris.is_empty() {
            return;
        }
        if self.active_queue && self.track_uris.get(index) == Some(&self.playing) {
            services.playback.toggle();
            return;
        }
        services.playback.play_queue(self.track_uris.clone(), index);
        self.active_queue = true;
        self.is_playing = true;
        if let Some(uri) = self.track_uris.get(index) {
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
        self.active_queue = same_queue(&self.track_uris, &queue);
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

    fn card(
        &mut self,
        request: u64,
        picture: Option<&str>,
        round: bool,
        name: &str,
        sub: &str,
        sender: &ComponentSender<Self>,
    ) -> gtk::Button {
        let art = self.art(request, 104, round, picture, sender);

        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.append(&art);
        let name_label = gtk::Label::new(Some(name));
        name_label.add_css_class("card-name");
        name_label.set_xalign(0.0);
        name_label.set_max_width_chars(12);
        name_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        body.append(&name_label);
        let sub_label = gtk::Label::new(Some(sub));
        sub_label.add_css_class("card-sub");
        sub_label.set_xalign(0.0);
        sub_label.set_max_width_chars(14);
        sub_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        body.append(&sub_label);

        let card = gtk::Button::builder().child(&body).build();
        card.add_css_class("card");

        card
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
        self.load_image(request, picture, display, sender);

        tile
    }

    fn load_image(
        &mut self,
        request: u64,
        picture: &str,
        display: gtk::Picture,
        sender: &ComponentSender<Self>,
    ) {
        if let Some(path) = images::cached(picture) {
            display.set_filename(Some(&path));
            return;
        }
        self.thumbs.push((picture.to_owned(), display));
        let Some(services) = self.services.clone() else {
            return;
        };
        let picture = picture.to_owned();
        let mut requests = self.requests.subscribe();
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    tokio::select! {
                        result = images::fetch(&services.session, &picture) => {
                            if let Ok(path) = result {
                                let _ = out.send(SearchCmd::Image(request, picture, path));
                            }
                        }
                        _ = requests.changed() => {}
                    }
                })
                .drop_on_shutdown()
        });
    }
}

enum Pick {
    Artist(SearchArtist),
    Track(usize, SearchTrack),
    Album(SearchAlbum),
    Playlist(SearchPlaylist),
}

fn score(name: &str, query: &str) -> u64 {
    let name = name.to_lowercase();
    if name == query {
        60
    } else if name.starts_with(query) {
        40
    } else if name.contains(query) {
        20
    } else {
        0
    }
}

fn section(title: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(title));
    label.add_css_class("search-section");
    label.set_xalign(0.0);

    label
}

fn scroller(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(child)
        .build();
    let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    wheel.connect_scroll({
        let adjustment = scroller.hadjustment();
        move |controller, dx, dy| {
            if adjustment.upper() - adjustment.page_size() <= 1.0 {
                return gtk::glib::Propagation::Proceed;
            }
            let delta = if dx.abs() > dy.abs() { dx } else { dy };
            let step = if controller.unit() == gtk::gdk::ScrollUnit::Wheel {
                delta * 120.0
            } else {
                delta
            };
            adjustment.set_value(adjustment.value() + step);
            gtk::glib::Propagation::Stop
        }
    });
    scroller.add_controller(wheel);

    scroller
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
