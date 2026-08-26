use std::{cell::Cell, collections::HashSet, rc::Rc, sync::Arc, time::Duration};

use gatefold_core::{
    images, metadata,
    model::{PlaylistInfo, PlaylistRef},
    player,
};
use relm4::{Component, ComponentParts, ComponentSender, adw::prelude::*, gtk};

use crate::app::Services;

pub const CSS: &str = include_str!("style.css");

const HANDLE: i32 = 12;

pub struct PlaylistPage {
    services: Option<Arc<Services>>,
    listening: bool,
    requests: tokio::sync::watch::Sender<u64>,
    uri: String,
    playing: String,
    active_queue: bool,
    is_playing: bool,
    uris: Vec<String>,
    rows: Vec<(String, gtk::Widget, gtk::Image)>,
    equalizers: Vec<(String, [gtk::Box; 3])>,
    equalizer_phase: usize,
    thumbs: Vec<(String, gtk::Image)>,
    play: gtk::Button,
    play_icon: gtk::Image,
    play_label: gtk::Label,
    cover: gtk::Picture,
    title: gtk::Label,
    owner: gtk::Label,
    detail: gtk::Label,
    blurb: gtk::Label,
    release: gtk::Label,
    shelf: gtk::Box,
}

pub enum PlaylistAction {
    Show(Arc<Services>, PlaylistRef),
    Primary,
    ShufflePlay,
    PlayFrom(usize),
    Pulse,
}

#[derive(Debug)]
pub enum PlaylistOutput {
    Cover(std::path::PathBuf),
}

impl std::fmt::Debug for PlaylistAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaylistAction::Show(_, playlist) => write!(f, "Show({})", playlist.name),
            PlaylistAction::Primary => write!(f, "Primary"),
            PlaylistAction::ShufflePlay => write!(f, "ShufflePlay"),
            PlaylistAction::PlayFrom(index) => write!(f, "PlayFrom({index})"),
            PlaylistAction::Pulse => write!(f, "Pulse"),
        }
    }
}

#[derive(Debug)]
pub enum PlaylistCmd {
    Loaded(u64, Box<PlaylistInfo>),
    Cover(u64, std::path::PathBuf),
    TrackCover(u64, String, std::path::PathBuf),
    Playback(player::Event),
    Failed(String),
}

impl Component for PlaylistPage {
    type Init = ();
    type Input = PlaylistAction;
    type Output = PlaylistOutput;
    type CommandOutput = PlaylistCmd;
    type Root = gtk::Paned;
    type Widgets = ();

    fn init_root() -> Self::Root {
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.add_css_class("gatefold");
        paned.set_shrink_start_child(false);
        paned.set_shrink_end_child(false);
        paned.set_resize_start_child(false);
        paned.set_resize_end_child(true);

        paned
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let inner = gtk::Box::new(gtk::Orientation::Vertical, 0);
        inner.add_css_class("sleeve-inner");

        let cover = gtk::Picture::new();
        cover.set_content_fit(gtk::ContentFit::Cover);
        cover.set_can_shrink(true);
        cover.add_css_class("header-art");
        cover.set_overflow(gtk::Overflow::Hidden);

        let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        holder.set_margin_bottom(20);
        holder.set_layout_manager(Some(crate::square::SquareLayout::new()));
        holder.append(&cover);
        inner.append(&holder);

        let title = gtk::Label::new(None);
        title.add_css_class("page-title");
        title.set_xalign(0.0);
        title.set_wrap(true);
        inner.append(&title);

        let meta = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        meta.add_css_class("page-meta");
        let owner = gtk::Label::new(None);
        owner.add_css_class("page-owner");
        owner.set_xalign(0.0);
        owner.set_ellipsize(gtk::pango::EllipsizeMode::End);
        meta.append(&owner);
        let detail = gtk::Label::new(None);
        detail.add_css_class("page-detail");
        detail.set_xalign(0.0);
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        meta.append(&detail);
        inner.append(&meta);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        actions.add_css_class("actions");
        actions.set_margin_top(16);
        let play_icon = gtk::Image::from_icon_name("media-playback-start-symbolic");
        let play_label = gtk::Label::new(Some("Play"));
        let play_content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        play_content.append(&play_icon);
        play_content.append(&play_label);
        let play = gtk::Button::builder().child(&play_content).build();
        play.add_css_class("pill");
        play.add_css_class("filled");
        play.set_margin_end(8);
        play.set_tooltip_text(Some("Play playlist"));
        let on_play = sender.input_sender().clone();
        play.connect_clicked(move |_| on_play.emit(PlaylistAction::Primary));
        actions.append(&play);
        let shuffle = gtk::Button::from_icon_name("media-playlist-shuffle-symbolic");
        shuffle.add_css_class("icon");
        shuffle.set_valign(gtk::Align::Center);
        shuffle.set_tooltip_text(Some("Shuffle play"));
        let on_shuffle = sender.input_sender().clone();
        shuffle.connect_clicked(move |_| on_shuffle.emit(PlaylistAction::ShufflePlay));
        actions.append(&shuffle);
        let save = gtk::Button::from_icon_name("list-add-symbolic");
        save.add_css_class("icon");
        save.set_valign(gtk::Align::Center);
        save.set_tooltip_text(Some("Save to your library"));
        actions.append(&save);
        let more = gtk::Button::from_icon_name("view-more-symbolic");
        more.add_css_class("icon");
        more.set_valign(gtk::Align::Center);
        more.set_tooltip_text(Some("More options"));
        actions.append(&more);
        inner.append(&actions);

        let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        spacer.set_vexpand(true);
        inner.append(&spacer);

        let blurb = gtk::Label::new(None);
        blurb.add_css_class("page-detail");
        blurb.set_xalign(0.0);
        blurb.set_wrap(true);
        blurb.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        blurb.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
        blurb.set_max_width_chars(36);
        blurb.set_margin_top(20);
        blurb.set_visible(false);
        inner.append(&blurb);

        let release = gtk::Label::new(None);
        release.add_css_class("release-line");
        release.set_xalign(0.0);
        release.set_ellipsize(gtk::pango::EllipsizeMode::End);
        release.set_hexpand(true);
        release.set_valign(gtk::Align::End);

        let flip = gtk::Button::from_icon_name("object-flip-horizontal-symbolic");
        flip.add_css_class("icon");
        flip.set_valign(gtk::Align::End);
        flip.set_tooltip_text(Some("Swap sides"));

        let foot = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        foot.append(&release);
        foot.append(&flip);
        inner.append(&foot);

        let sleeve = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sleeve.add_css_class("sleeve");
        sleeve.set_overflow(gtk::Overflow::Hidden);
        sleeve.set_size_request(280, -1);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(false)
            .vexpand(true)
            .child(&inner)
            .build();
        sleeve.append(&scroll);
        root.set_start_child(Some(&sleeve));

        let shelf = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let notes = gtk::Box::new(gtk::Orientation::Vertical, 0);
        notes.add_css_class("notes");
        notes.set_hexpand(true);
        let list = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(false)
            .vexpand(true)
            .child(&shelf)
            .build();
        notes.append(&list);
        root.set_end_child(Some(&notes));
        root.set_position(320);

        let flipped = Rc::new(Cell::new(false));
        let busy = Rc::new(Cell::new(false));
        let paned = root.clone();
        let animated_sleeve = sleeve.clone();
        let animated_notes = notes.clone();
        flip.connect_clicked(move |_| {
            if busy.replace(true) {
                return;
            }

            let width = paned.width();
            let position = paned.position();
            let was_flipped = flipped.get();
            let sleeve_min = animated_sleeve.preferred_size().0.width();
            let notes_min = animated_notes.preferred_size().0.width();
            let sleeve_width = if was_flipped {
                width - HANDLE - position
            } else {
                position
            }
            .clamp(sleeve_min, width - HANDLE - notes_min);
            let closed = if was_flipped { width - HANDLE } else { 0 };

            animated_sleeve.set_size_request(sleeve_width, -1);
            paned.set_shrink_start_child(true);
            paned.set_shrink_end_child(true);

            let slide = relm4::adw::CallbackAnimationTarget::new({
                let paned = paned.clone();
                move |value| paned.set_position(value as i32)
            });
            let close =
                relm4::adw::TimedAnimation::new(&paned, position as f64, closed as f64, 220, slide);
            close.set_easing(relm4::adw::Easing::EaseInCubic);

            let p = paned.clone();
            let s = animated_sleeve.clone();
            let n = animated_notes.clone();
            let f = flipped.clone();
            let b = busy.clone();
            close.connect_done(move |_| {
                p.set_start_child(None::<&gtk::Widget>);
                p.set_end_child(None::<&gtk::Widget>);

                let now_flipped = !was_flipped;
                f.set(now_flipped);
                if now_flipped {
                    p.set_start_child(Some(&n));
                    p.set_end_child(Some(&s));
                } else {
                    p.set_start_child(Some(&s));
                    p.set_end_child(Some(&n));
                }
                p.set_resize_start_child(now_flipped);
                p.set_resize_end_child(!now_flipped);

                let (from, to) = if now_flipped {
                    (width - HANDLE, width - HANDLE - sleeve_width)
                } else {
                    (0, sleeve_width)
                };
                p.set_position(from);

                let slide = relm4::adw::CallbackAnimationTarget::new({
                    let paned = p.clone();
                    move |value| paned.set_position(value as i32)
                });
                let open = relm4::adw::TimedAnimation::new(&p, from as f64, to as f64, 260, slide);
                open.set_easing(relm4::adw::Easing::EaseOutCubic);
                let p = p.clone();
                let b = b.clone();
                let s = s.clone();
                open.connect_done(move |_| {
                    s.set_size_request(-1, -1);
                    p.set_shrink_start_child(false);
                    p.set_shrink_end_child(false);
                    b.set(false);
                });
                open.play();
            });
            close.play();
        });

        let (requests, _) = tokio::sync::watch::channel(0);
        let model = PlaylistPage {
            services: None,
            listening: false,
            requests,
            uri: String::new(),
            playing: String::new(),
            active_queue: false,
            is_playing: false,
            uris: Vec::new(),
            rows: Vec::new(),
            equalizers: Vec::new(),
            equalizer_phase: 0,
            thumbs: Vec::new(),
            play,
            play_icon,
            play_label,
            cover,
            title,
            owner,
            detail,
            blurb,
            release,
            shelf,
        };

        let pulse = sender.input_sender().clone();
        gtk::glib::timeout_add_local(Duration::from_millis(180), move || {
            pulse.emit(PlaylistAction::Pulse);
            gtk::glib::ControlFlow::Continue
        });

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            PlaylistAction::Show(services, playlist) => self.show(services, playlist, &sender),
            PlaylistAction::Primary => self.primary(),
            PlaylistAction::ShufflePlay => self.play(0, true),
            PlaylistAction::PlayFrom(index) => self.play_from(index),
            PlaylistAction::Pulse => self.pulse(),
        }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            PlaylistCmd::Loaded(request, playlist) => {
                if request != *self.requests.borrow() {
                    return;
                }
                self.blurb.set_text(&playlist.description);
                self.blurb.set_visible(!playlist.description.is_empty());
                self.owner.set_text(&playlist.owner);
                let duration_ms: u64 = playlist
                    .tracks
                    .iter()
                    .map(|track| u64::from(track.duration_ms))
                    .sum();
                self.detail.set_text(&format!(
                    " · {} songs · {} min",
                    playlist.tracks.len(),
                    duration_ms / 60_000
                ));
                self.release.set_text(
                    &format_updated(playlist.updated_at_ms)
                        .unwrap_or_else(|| format!("Playlist by {}", self.owner.text())),
                );
                self.uris = playlist
                    .tracks
                    .iter()
                    .map(|track| track.uri.clone())
                    .collect();
                self.sync_playback();

                self.rows.clear();
                self.equalizers.clear();
                self.thumbs.clear();
                while let Some(child) = self.shelf.first_child() {
                    self.shelf.remove(&child);
                }
                let primary_artist = playlist
                    .tracks
                    .first()
                    .and_then(|track| track.artists.first())
                    .map(|artist| artist.name.as_str());
                let mixed_artists = playlist.tracks.iter().any(|track| {
                    track.artists.first().map(|artist| artist.name.as_str()) != primary_artist
                });
                let mut requested_covers = HashSet::new();
                for (index, track) in playlist.tracks.iter().enumerate() {
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
                    let bars = std::array::from_fn(|_| {
                        let bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
                        bar.add_css_class("track-equalizer-bar");
                        bar.set_size_request(3, 6);
                        bar.set_valign(gtk::Align::End);
                        equalizer.append(&bar);
                        bar
                    });
                    leading.add_overlay(&equalizer);
                    row.append(&leading);

                    if mixed_artists {
                        let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        tile.add_css_class("track-art");
                        tile.set_size_request(40, 40);
                        tile.set_valign(gtk::Align::Center);
                        tile.set_hexpand(false);
                        tile.set_overflow(gtk::Overflow::Hidden);
                        let cached = track.cover_id.as_deref().and_then(images::cached);
                        let image = match &cached {
                            Some(path) => {
                                let image = gtk::Image::from_file(path);
                                image.set_pixel_size(40);
                                image
                            }
                            None => gtk::Image::from_icon_name("emblem-music-symbolic"),
                        };
                        image.set_halign(gtk::Align::Center);
                        image.set_hexpand(true);
                        tile.append(&image);
                        row.append(&tile);

                        if let Some(cover_id) = &track.cover_id {
                            self.thumbs.push((cover_id.clone(), image));
                            if cached.is_none()
                                && requested_covers.insert(cover_id.clone())
                                && let Some(services) = self.services.clone()
                            {
                                let mut requests = self.requests.subscribe();
                                let cover_id = cover_id.clone();
                                sender.command(move |out, shutdown| {
                                    shutdown
                                        .register(async move {
                                            tokio::select! {
                                                result = images::fetch(&services.session, &cover_id) => {
                                                    if let Ok(path) = result {
                                                        let _ = out.send(PlaylistCmd::TrackCover(
                                                            request,
                                                            cover_id,
                                                            path,
                                                        ));
                                                    }
                                                }
                                                _ = requests.changed() => {}
                                            }
                                        })
                                        .drop_on_shutdown()
                                });
                            }
                        }
                    }

                    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
                    text.set_valign(gtk::Align::Center);
                    text.set_hexpand(true);
                    let name = gtk::Label::new(Some(&track.name));
                    name.add_css_class("track-name");
                    name.set_xalign(0.0);
                    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    text.append(&name);
                    let artists: Vec<&str> = track
                        .artists
                        .iter()
                        .map(|artist| artist.name.as_str())
                        .collect();
                    let artists = gtk::Label::new(Some(&artists.join(", ")));
                    artists.add_css_class("track-artists");
                    artists.set_xalign(0.0);
                    artists.set_ellipsize(gtk::pango::EllipsizeMode::End);
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
                    let on_row = sender.input_sender().clone();
                    button.connect_clicked(move |_| on_row.emit(PlaylistAction::PlayFrom(index)));
                    self.rows
                        .push((track.uri.clone(), button.clone().upcast(), track_play));
                    self.equalizers.push((track.uri.clone(), bars));
                    self.shelf.append(&button);
                }
            }
            PlaylistCmd::Cover(request, path) => {
                if request == *self.requests.borrow() {
                    self.cover.set_filename(Some(&path));
                    let _ = sender.output(PlaylistOutput::Cover(path));
                }
            }
            PlaylistCmd::TrackCover(request, cover_id, path) => {
                if request != *self.requests.borrow() {
                    return;
                }
                for (thumb_id, image) in &self.thumbs {
                    if thumb_id == &cover_id {
                        image.set_from_file(Some(&path));
                        image.set_pixel_size(40);
                    }
                }
            }
            PlaylistCmd::Playback(event) => self.playback(event),
            PlaylistCmd::Failed(error) => tracing::error!("{error}"),
        }
    }

    fn update_view(&self, _: &mut Self::Widgets, _: ComponentSender<Self>) {}
}

impl PlaylistPage {
    fn show(
        &mut self,
        services: Arc<Services>,
        playlist: PlaylistRef,
        sender: &ComponentSender<Self>,
    ) {
        let request = (*self.requests.borrow()).wrapping_add(1);
        self.requests.send_replace(request);
        self.uri = playlist.uri.clone();
        self.title.set_text(&playlist.name);
        self.owner.set_text(&playlist.owner);
        self.detail
            .set_text(&format!(" · {} songs", playlist.length));
        self.release
            .set_text(&format!("Playlist by {}", playlist.owner));
        self.blurb.set_visible(false);
        self.cover.set_paintable(gtk::gdk::Paintable::NONE);
        self.active_queue = false;
        self.refresh_play_button();
        self.uris.clear();
        self.rows.clear();
        self.equalizers.clear();
        self.thumbs.clear();
        while let Some(child) = self.shelf.first_child() {
            self.shelf.remove(&child);
        }

        if let Some(picture) = playlist.picture.clone() {
            if let Some(path) = images::cached(&picture) {
                self.cover.set_filename(Some(&path));
                let _ = sender.output(PlaylistOutput::Cover(path));
            } else {
                let session = services.session.clone();
                let mut requests = self.requests.subscribe();
                sender.command(move |out, shutdown| {
                    shutdown
                        .register(async move {
                            tokio::select! {
                                result = images::fetch(&session, &picture) => {
                                    let message = match result {
                                        Ok(path) => PlaylistCmd::Cover(request, path),
                                        Err(error) => PlaylistCmd::Failed(format!("playlist cover: {error}")),
                                    };
                                    let _ = out.send(message);
                                }
                                _ = requests.changed() => {}
                            }
                        })
                        .drop_on_shutdown()
                });
            }
        }

        let session = services.session.clone();
        let uri = playlist.uri.clone();
        let mut requests = self.requests.subscribe();
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    tokio::select! {
                        result = metadata::playlist(&session, &uri) => {
                            let message = match result {
                                Ok(playlist) => PlaylistCmd::Loaded(request, Box::new(playlist)),
                                Err(error) => PlaylistCmd::Failed(format!("playlist: {error}")),
                            };
                            let _ = out.send(message);
                        }
                        _ = requests.changed() => {}
                    }
                })
                .drop_on_shutdown()
        });

        if !self.listening {
            self.listening = true;
            let mut events = services.playback.events();
            sender.command(|out, shutdown| {
                shutdown
                    .register(async move {
                        while let Ok(event) = events.recv().await {
                            let _ = out.send(PlaylistCmd::Playback(event));
                        }
                    })
                    .drop_on_shutdown()
            });
        }

        self.services = Some(services);
    }

    fn primary(&mut self) {
        if self.active_queue {
            if let Some(services) = &self.services {
                services.playback.toggle();
            }
        } else {
            self.play(0, false);
        }
    }

    fn play(&mut self, index: usize, shuffle: bool) {
        let Some(services) = &self.services else {
            return;
        };
        if self.uris.is_empty() {
            return;
        }
        services.playback.play_queue(self.uris.clone(), index);
        services.playback.set_shuffle(shuffle);
        self.active_queue = true;
        self.refresh_play_button();
    }

    fn play_from(&mut self, index: usize) {
        if self.active_queue && self.uris.get(index) == Some(&self.playing) {
            if let Some(services) = &self.services {
                services.playback.toggle();
            }
        } else {
            self.play(index, false);
        }
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
        self.refresh_play_button();
    }

    fn pulse(&mut self) {
        const HEIGHTS: [[i32; 3]; 4] = [[5, 12, 8], [10, 6, 13], [13, 9, 5], [7, 13, 10]];

        if !self.active_queue || !self.is_playing {
            return;
        }
        self.equalizer_phase = (self.equalizer_phase + 1) % HEIGHTS.len();
        for (uri, bars) in &self.equalizers {
            if *uri == self.playing {
                for (bar, height) in bars.iter().zip(HEIGHTS[self.equalizer_phase]) {
                    bar.set_size_request(3, height);
                }
            }
        }
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
        self.refresh_play_button();
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

    fn refresh_play_button(&self) {
        let playing = self.active_queue && self.is_playing;
        self.play_icon.set_icon_name(Some(if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        }));
        self.play_label
            .set_text(if playing { "Pause" } else { "Play" });
        self.play.set_tooltip_text(Some(if playing {
            "Pause playlist"
        } else {
            "Play playlist"
        }));
        if playing {
            self.play.add_css_class("playing");
        } else {
            self.play.remove_css_class("playing");
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

fn format_updated(timestamp_ms: i64) -> Option<String> {
    if timestamp_ms <= 0 {
        return None;
    }
    let date = gtk::glib::DateTime::from_unix_local(timestamp_ms / 1000).ok()?;
    let formatted = date.format("%e %B %Y").ok()?;
    Some(format!("Updated {}", formatted.trim_start()))
}
