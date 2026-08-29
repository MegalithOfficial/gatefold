use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

use gatefold_core::{
    images, metadata,
    model::{ArtistRef, TrackInfo},
    player::{self, Snapshot},
};
use relm4::{Component, ComponentParts, ComponentSender, gtk, gtk::prelude::*};

use crate::{
    app::Services,
    artists,
    menu::{self, TrackMenu},
    skeleton,
};

pub const CSS: &str = include_str!("style.css");

const QUICK_ROWS: usize = 8;

#[derive(Debug, Clone, Copy)]
enum UpNextMenu {
    Remove,
}

const UP_NEXT: &[(&str, UpNextMenu)] = &[("Remove from queue", UpNextMenu::Remove)];

pub struct QueuePage {
    services: Option<Arc<Services>>,
    snapshot: Snapshot,
    known: HashMap<String, TrackInfo>,
    pending: HashSet<String>,
    is_playing: bool,
    column: gtk::Box,
    quick: gtk::Box,
    now: Vec<(gtk::Button, gtk::Image)>,
    thumbs: Vec<(String, gtk::Image, i32)>,
}

pub enum QueueAction {
    SetServices(Arc<Services>),
    Toggle,
    PlayUpNext(usize),
    Jump(usize),
    Remove(usize),
    Clear,
    Enqueue(String, TrackMenu),
    OpenArtist(Box<ArtistRef>),
    OpenPage,
}

impl std::fmt::Debug for QueueAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueueAction::SetServices(_) => write!(f, "SetServices"),
            QueueAction::Toggle => write!(f, "Toggle"),
            QueueAction::PlayUpNext(index) => write!(f, "PlayUpNext({index})"),
            QueueAction::Jump(position) => write!(f, "Jump({position})"),
            QueueAction::Remove(index) => write!(f, "Remove({index})"),
            QueueAction::Clear => write!(f, "Clear"),
            QueueAction::Enqueue(uri, pick) => write!(f, "Enqueue({uri}, {pick:?})"),
            QueueAction::OpenArtist(artist) => write!(f, "OpenArtist({})", artist.name),
            QueueAction::OpenPage => write!(f, "OpenPage"),
        }
    }
}

#[derive(Debug)]
pub enum QueueOutput {
    OpenArtist(Box<ArtistRef>),
    OpenPage,
}

#[derive(Debug)]
pub enum QueueCmd {
    Playback(player::Event),
    Tracks(Vec<String>, Vec<TrackInfo>),
    Cover(String, PathBuf),
}

#[relm4::component(pub)]
impl Component for QueuePage {
    type Init = gtk::Box;
    type Input = QueueAction;
    type Output = QueueOutput;
    type CommandOutput = QueueCmd;

    view! {
        gtk::ScrolledWindow {
            add_css_class: "queue-page",
            set_hscrollbar_policy: gtk::PolicyType::Never,
            set_hexpand: true,
            set_vexpand: true,

            #[local_ref]
            column -> gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                add_css_class: "queue-column",
            },
        }
    }

    fn init(
        quick: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        quick.set_orientation(gtk::Orientation::Vertical);
        quick.add_css_class("queue-quick");
        quick.set_width_request(340);
        let mut model = QueuePage {
            services: None,
            snapshot: Snapshot::default(),
            known: HashMap::new(),
            pending: HashSet::new(),
            is_playing: false,
            column: gtk::Box::new(gtk::Orientation::Vertical, 0),
            quick,
            now: Vec::new(),
            thumbs: Vec::new(),
        };
        let column = &model.column;
        let widgets = view_output!();
        let _ = root;
        model.render(&sender);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        if let QueueAction::SetServices(services) = action {
            let mut events = services.playback.events();
            self.services = Some(services);
            sender.command(|out, shutdown| {
                shutdown
                    .register(async move {
                        while let Ok(event) = events.recv().await {
                            let _ = out.send(QueueCmd::Playback(event));
                        }
                    })
                    .drop_on_shutdown()
            });
            self.refresh(&sender);
            return;
        }

        let Some(services) = &self.services else {
            return;
        };
        let playback = &services.playback;

        match action {
            QueueAction::Toggle => playback.toggle(),
            QueueAction::PlayUpNext(index) => playback.play_up_next(index),
            QueueAction::Jump(position) => playback.jump(position),
            QueueAction::Remove(index) => playback.remove_up_next(index),
            QueueAction::Clear => playback.clear_up_next(),
            QueueAction::Enqueue(uri, pick) => menu::enqueue(playback, &uri, pick),
            QueueAction::OpenArtist(artist) => {
                let _ = sender.output(QueueOutput::OpenArtist(artist));
            }
            QueueAction::OpenPage => {
                let _ = sender.output(QueueOutput::OpenPage);
            }
            QueueAction::SetServices(_) => {}
        }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            QueueCmd::Playback(event) => match event {
                player::Event::Loading { .. }
                | player::Event::TrackChanged { .. }
                | player::Event::QueueChanged { .. }
                | player::Event::UpNextChanged
                | player::Event::Stopped => self.refresh(&sender),
                player::Event::Playing { .. } => self.set_playing(true),
                player::Event::Paused { .. } => self.set_playing(false),
                _ => {}
            },
            QueueCmd::Tracks(requested, tracks) => {
                for uri in requested {
                    self.pending.remove(&uri);
                }
                for track in tracks {
                    self.known.insert(track.uri.clone(), track);
                }
                self.render(&sender);
            }
            QueueCmd::Cover(cover_id, path) => {
                for (thumb_id, image, size) in &self.thumbs {
                    if thumb_id == &cover_id {
                        image.set_from_file(Some(&path));
                        image.set_pixel_size(*size);
                    }
                }
            }
        }
    }
}

impl QueuePage {
    fn refresh(&mut self, sender: &ComponentSender<Self>) {
        let Some(services) = self.services.clone() else {
            return;
        };
        self.snapshot = services.playback.snapshot();
        self.is_playing = services.playback.is_playing();

        let missing: Vec<String> = self
            .snapshot
            .current
            .iter()
            .chain(&self.snapshot.up_next)
            .chain(&self.snapshot.ahead)
            .filter(|uri| !self.known.contains_key(*uri) && !self.pending.contains(*uri))
            .cloned()
            .collect();
        if !missing.is_empty() {
            self.pending.extend(missing.iter().cloned());
            let session = services.session();
            sender.oneshot_command(async move {
                let tracks = metadata::tracks(&session, missing.clone()).await;
                QueueCmd::Tracks(missing, tracks)
            });
        }
        self.render(sender);
    }

    fn set_playing(&mut self, playing: bool) {
        self.is_playing = playing;
        for (row, icon) in &self.now {
            if playing {
                row.add_css_class("playing-active");
            } else {
                row.remove_css_class("playing-active");
            }
            icon.set_icon_name(Some(if playing {
                "media-playback-pause-symbolic"
            } else {
                "media-playback-start-symbolic"
            }));
        }
    }

    fn render(&mut self, sender: &ComponentSender<Self>) {
        self.thumbs.clear();
        self.now.clear();
        self.render_page(sender);
        self.render_quick(sender);
    }

    fn render_page(&mut self, sender: &ComponentSender<Self>) {
        let column = self.column.clone();
        while let Some(child) = column.first_child() {
            column.remove(&child);
        }

        let title = gtk::Label::new(Some("Queue"));
        title.add_css_class("queue-title");
        title.set_xalign(0.0);
        column.append(&title);

        let snapshot = self.snapshot.clone();
        if snapshot.current.is_none() && snapshot.up_next.is_empty() {
            let empty = gtk::Label::new(Some("Play something to build a queue"));
            empty.add_css_class("queue-empty");
            empty.set_xalign(0.0);
            column.append(&empty);
            return;
        }

        let mut requested = HashSet::new();
        if let Some(uri) = &snapshot.current {
            column.append(&head("Now playing", None));
            let enqueue = sender.input_sender().clone();
            let queued = uri.clone();
            if let Some(row) = self.row(&column, uri, 0, sender, &mut requested, true) {
                let toggle = sender.input_sender().clone();
                row.connect_clicked(move |_| toggle.emit(QueueAction::Toggle));
                row.append_more(menu::attach(&row.button, menu::TRACK, move |pick| {
                    enqueue.emit(QueueAction::Enqueue(queued.clone(), pick));
                }));
            }
        }

        if !snapshot.up_next.is_empty() {
            let clear = gtk::Button::with_label("Clear");
            clear.add_css_class("queue-clear");
            let on_clear = sender.input_sender().clone();
            clear.connect_clicked(move |_| on_clear.emit(QueueAction::Clear));
            column.append(&head("Next in queue", Some(&clear)));
            for (index, uri) in snapshot.up_next.iter().enumerate() {
                if let Some(row) = self.row(&column, uri, index, sender, &mut requested, false) {
                    let play = sender.input_sender().clone();
                    row.connect_clicked(move |_| play.emit(QueueAction::PlayUpNext(index)));
                    let remove = sender.input_sender().clone();
                    row.append_more(menu::attach(
                        &row.button,
                        UP_NEXT,
                        move |UpNextMenu::Remove| {
                            remove.emit(QueueAction::Remove(index));
                        },
                    ));
                }
            }
        }

        if !snapshot.ahead.is_empty() {
            column.append(&head(&next_from(&snapshot.source), None));
            for (index, uri) in snapshot.ahead.iter().enumerate() {
                if let Some(row) = self.row(&column, uri, index, sender, &mut requested, false) {
                    let position = snapshot.ahead_from + index;
                    let jump = sender.input_sender().clone();
                    row.connect_clicked(move |_| jump.emit(QueueAction::Jump(position)));
                    let enqueue = sender.input_sender().clone();
                    let queued = uri.clone();
                    row.append_more(menu::attach(&row.button, menu::TRACK, move |pick| {
                        enqueue.emit(QueueAction::Enqueue(queued.clone(), pick));
                    }));
                }
            }
        }
    }

    fn render_quick(&mut self, sender: &ComponentSender<Self>) {
        let quick = self.quick.clone();
        while let Some(child) = quick.first_child() {
            quick.remove(&child);
        }

        let open = gtk::Button::with_label("Open queue");
        open.add_css_class("queue-clear");
        let on_open = sender.input_sender().clone();
        open.connect_clicked(move |_| on_open.emit(QueueAction::OpenPage));
        let title = head("Queue", Some(&open));
        title.add_css_class("queue-quick-title");
        quick.append(&title);

        let snapshot = self.snapshot.clone();
        if snapshot.current.is_none() && snapshot.up_next.is_empty() {
            let empty = gtk::Label::new(Some("Nothing queued"));
            empty.add_css_class("queue-empty");
            empty.set_xalign(0.0);
            quick.append(&empty);
            return;
        }

        let mut requested = HashSet::new();
        let mut left = QUICK_ROWS;
        if let Some(uri) = &snapshot.current {
            let toggle = sender.input_sender().clone();
            self.quick_row(&quick, uri, sender, &mut requested, true, move || {
                toggle.emit(QueueAction::Toggle);
            });
            left -= 1;
        }
        if !snapshot.up_next.is_empty() {
            quick.append(&quick_head("Next in queue"));
            for (index, uri) in snapshot.up_next.iter().take(left).enumerate() {
                let play = sender.input_sender().clone();
                self.quick_row(&quick, uri, sender, &mut requested, false, move || {
                    play.emit(QueueAction::PlayUpNext(index));
                });
            }
            left = left.saturating_sub(snapshot.up_next.len());
        }
        if left > 0 && !snapshot.ahead.is_empty() {
            quick.append(&quick_head(&next_from(&snapshot.source)));
            for (index, uri) in snapshot.ahead.iter().take(left).enumerate() {
                let position = snapshot.ahead_from + index;
                let jump = sender.input_sender().clone();
                self.quick_row(&quick, uri, sender, &mut requested, false, move || {
                    jump.emit(QueueAction::Jump(position));
                });
            }
        }
    }

    fn row(
        &mut self,
        column: &gtk::Box,
        uri: &str,
        index: usize,
        sender: &ComponentSender<Self>,
        requested: &mut HashSet<String>,
        now: bool,
    ) -> Option<Row> {
        let Some(track) = self.known.get(uri).cloned() else {
            let bones = skeleton::track_row(index as i32, false);
            bones.set_margin_start(16);
            bones.set_margin_end(16);
            column.append(&bones);
            return None;
        };

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);

        let leading = gtk::Overlay::new();
        leading.add_css_class("track-leading");
        leading.set_size_request(20, -1);
        leading.set_child(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
        let track_play = gtk::Image::from_icon_name(if now && self.is_playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
        track_play.add_css_class("track-play");
        track_play.set_halign(gtk::Align::Center);
        track_play.set_valign(gtk::Align::Center);
        leading.add_overlay(&track_play);
        if now {
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
        }
        row.append(&leading);

        row.append(&self.art(&track, 40, sender, requested));

        let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
        text.set_valign(gtk::Align::Center);
        text.set_hexpand(true);
        let name = gtk::Label::new(Some(&track.name));
        name.add_css_class("track-name");
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&name);
        let open = sender.input_sender().clone();
        text.append(&artists::label(&track.artists, move |artist| {
            open.emit(QueueAction::OpenArtist(Box::new(artist)));
        }));
        row.append(&text);

        let time = gtk::Label::new(Some(&clock(track.duration_ms)));
        time.add_css_class("track-time");
        row.append(&time);

        let button = gtk::Button::builder().child(&row).build();
        button.add_css_class("track");
        if now {
            if self.is_playing {
                button.add_css_class("playing-active");
            }
            self.now.push((button.clone(), track_play));
        }
        column.append(&button);

        Some(Row { button, row })
    }

    fn quick_row(
        &mut self,
        quick: &gtk::Box,
        uri: &str,
        sender: &ComponentSender<Self>,
        requested: &mut HashSet<String>,
        now: bool,
        activate: impl Fn() + 'static,
    ) {
        let Some(track) = self.known.get(uri).cloned() else {
            let bones = skeleton::track_row(0, false);
            bones.set_margin_start(10);
            bones.set_margin_end(10);
            quick.append(&bones);
            return;
        };

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        content.append(&self.art(&track, 36, sender, requested));
        let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text.set_valign(gtk::Align::Center);
        text.set_hexpand(true);
        let name = gtk::Label::new(Some(&track.name));
        name.add_css_class("quick-name");
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&name);
        let sub = gtk::Label::new(Some(&artists::names(&track.artists)));
        sub.add_css_class("quick-sub");
        sub.set_xalign(0.0);
        sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&sub);
        content.append(&text);
        if now {
            let icon = gtk::Image::from_icon_name(if self.is_playing {
                "media-playback-pause-symbolic"
            } else {
                "media-playback-start-symbolic"
            });
            icon.add_css_class("queue-quick-state");
            content.append(&icon);
        }

        let button = gtk::Button::builder().child(&content).build();
        button.add_css_class("quick-row");
        button.set_focus_on_click(false);
        button.connect_clicked(move |_| activate());
        if now {
            let icon = content.last_child().and_downcast::<gtk::Image>();
            if let Some(icon) = icon {
                self.now.push((button.clone(), icon));
            }
        }
        quick.append(&button);
    }

    fn art(
        &mut self,
        track: &TrackInfo,
        size: i32,
        sender: &ComponentSender<Self>,
        requested: &mut HashSet<String>,
    ) -> gtk::Box {
        let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tile.add_css_class(if size == 40 { "track-art" } else { "quick-art" });
        tile.set_size_request(size, size);
        tile.set_valign(gtk::Align::Center);
        tile.set_hexpand(false);
        tile.set_overflow(gtk::Overflow::Hidden);
        let cached = track.cover_id.as_deref().and_then(images::cached);
        let image = match &cached {
            Some(path) => {
                let image = gtk::Image::from_file(path);
                image.set_pixel_size(size);
                image
            }
            None => gtk::Image::from_icon_name("emblem-music-symbolic"),
        };
        image.set_halign(gtk::Align::Center);
        image.set_hexpand(true);
        tile.append(&image);
        if let Some(cover_id) = &track.cover_id {
            self.thumbs.push((cover_id.clone(), image, size));
            if cached.is_none()
                && requested.insert(cover_id.clone())
                && let Some(services) = self.services.clone()
            {
                let cover_id = cover_id.clone();
                sender.oneshot_command(async move {
                    match images::fetch(&services.session(), &cover_id).await {
                        Ok(path) => QueueCmd::Cover(cover_id, path),
                        Err(error) => {
                            tracing::warn!("cover: {error}");
                            QueueCmd::Tracks(Vec::new(), Vec::new())
                        }
                    }
                });
            }
        }

        tile
    }
}

struct Row {
    button: gtk::Button,
    row: gtk::Box,
}

impl Row {
    fn connect_clicked(&self, handler: impl Fn(&gtk::Button) + 'static) {
        self.button.connect_clicked(handler);
    }

    fn append_more(&self, more: gtk::Button) {
        self.row.append(&more);
    }
}

fn head(text: &str, trailing: Option<&gtk::Button>) -> gtk::Box {
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    head.add_css_class("queue-head");
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    head.append(&label);
    if let Some(button) = trailing {
        head.append(button);
    }

    head
}

fn quick_head(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("queue-quick-head");
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);

    label
}

fn next_from(source: &str) -> String {
    if source.is_empty() {
        "Next up".to_owned()
    } else {
        format!("Next from {source}")
    }
}

fn clock(ms: u32) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
