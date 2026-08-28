use std::sync::Arc;

use gatefold_core::{
    images, metadata,
    model::{AlbumRef, ArtistInfo, ArtistRef, PlaylistRef, ReleaseGroup},
    player,
};
use relm4::{
    Component, ComponentParts, ComponentSender,
    gtk::{self, prelude::*},
};

use crate::{app::Services, pages::discography::View, palette::Palette, text};

pub const CSS: &str = include_str!("style.css");

const HERO_MIN: i32 = 360;
const GRID_FOLD: usize = 12;

pub struct ArtistPage {
    services: Option<Arc<Services>>,
    listening: bool,
    requests: tokio::sync::watch::Sender<u64>,
    artist: ArtistRef,
    playing: String,
    active_queue: bool,
    is_playing: bool,
    uris: Vec<String>,
    rows: Vec<(String, gtk::Widget, gtk::Image)>,
    thumbs: Vec<(String, gtk::Picture)>,
    folded: Vec<gtk::Widget>,
    shelves: Vec<(gtk::Button, gtk::Widget, ReleaseGroup)>,
    photo: gtk::Picture,
    poster: gtk::Box,
    tint: gtk::Picture,
    face: gtk::Picture,
    name: gtk::Label,
    sticky_name: gtk::Label,
    detail: gtk::Label,
    detail_bone: gtk::Box,
    play_icon: gtk::Image,
    play_label: gtk::Label,
    latest: gtk::Box,
    popular: gtk::Box,
    more: gtk::Button,
    filters: gtk::Box,
    discography: gtk::Box,
    fans: gtk::Box,
    related: gtk::FlowBox,
    blurb: gtk::Label,
    credit: gtk::Label,
    scroll: gtk::ScrolledWindow,
}

pub enum ArtistAction {
    Show(Arc<Services>, ArtistRef, Option<String>),
    Primary,
    Shuffle,
    PlayTrack(usize),
    ToggleMore,
    Filter(usize),
    OpenPlaylist(Box<PlaylistRef>),
    OpenArtist(Box<ArtistRef>, Option<String>),
    OpenDiscography(View),
    ShowAll,
}

impl std::fmt::Debug for ArtistAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtistAction::Show(_, artist, _) => write!(f, "Show({})", artist.name),
            ArtistAction::Primary => write!(f, "Primary"),
            ArtistAction::Shuffle => write!(f, "Shuffle"),
            ArtistAction::PlayTrack(index) => write!(f, "PlayTrack({index})"),
            ArtistAction::ToggleMore => write!(f, "ToggleMore"),
            ArtistAction::Filter(index) => write!(f, "Filter({index})"),
            ArtistAction::OpenPlaylist(playlist) => write!(f, "OpenPlaylist({})", playlist.name),
            ArtistAction::OpenArtist(artist, _) => write!(f, "OpenArtist({})", artist.name),
            ArtistAction::OpenDiscography(view) => write!(f, "OpenDiscography({})", view.title()),
            ArtistAction::ShowAll => write!(f, "ShowAll"),
        }
    }
}

#[derive(Debug)]
pub enum ArtistOutput {
    OpenPlaylist(Box<PlaylistRef>),
    OpenArtist(Box<ArtistRef>, Option<String>),
    OpenDiscography(Box<ArtistRef>, View),
    Cover(std::path::PathBuf),
}

#[derive(Debug)]
pub enum ArtistCmd {
    Loaded(u64, Box<ArtistInfo>),
    Portrait(u64, std::path::PathBuf),
    Banner(u64, std::path::PathBuf),
    Singles(u64, Vec<AlbumRef>),
    Image(u64, String, std::path::PathBuf),
    Playback(player::Event),
    Failed(String),
}

impl Component for ArtistPage {
    type Init = ();
    type Input = ArtistAction;
    type Output = ArtistOutput;
    type CommandOutput = ArtistCmd;
    type Root = gtk::Overlay;
    type Widgets = ();

    fn init_root() -> Self::Root {
        let stage = gtk::Overlay::new();
        stage.add_css_class("artist-hero");
        stage.set_overflow(gtk::Overflow::Hidden);
        stage.set_hexpand(true);

        stage
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let photo = gtk::Picture::new();
        photo.set_content_fit(gtk::ContentFit::Cover);
        photo.set_can_shrink(true);
        photo.set_hexpand(true);
        photo.set_vexpand(true);
        root.set_child(Some(&photo));

        let poster = gtk::Box::new(gtk::Orientation::Vertical, 0);
        poster.add_css_class("hero-skeleton");
        poster.set_can_target(false);
        root.add_overlay(&poster);

        let tint = gtk::Picture::new();
        tint.set_content_fit(gtk::ContentFit::Cover);
        tint.set_can_shrink(true);
        tint.set_can_target(false);
        tint.set_opacity(0.6);
        root.add_overlay(&tint);

        let shade = gtk::Box::new(gtk::Orientation::Vertical, 0);
        shade.add_css_class("shade");
        shade.set_can_target(false);
        shade.set_opacity(0.0);
        root.add_overlay(&shade);

        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let sky = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sky.set_size_request(-1, HERO_MIN);
        column.append(&sky);

        let head = gtk::Box::new(gtk::Orientation::Vertical, 0);
        head.add_css_class("hero-head");
        head.set_valign(gtk::Align::End);
        head.set_vexpand(true);
        let name = gtk::Label::new(None);
        name.add_css_class("hero-name");
        name.set_xalign(0.0);
        name.set_wrap(true);
        head.append(&name);
        let detail = gtk::Label::new(None);
        detail.add_css_class("hero-detail");
        detail.set_xalign(0.0);
        head.append(&detail);
        let detail_bone = bone(240, 12);
        detail_bone.add_css_class("hero-bone");
        detail_bone.set_halign(gtk::Align::Start);
        detail_bone.set_margin_top(4);
        detail_bone.set_margin_bottom(4);
        head.append(&detail_bone);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        actions.set_margin_top(20);
        let play_icon = gtk::Image::from_icon_name("media-playback-start-symbolic");
        let play_label = gtk::Label::new(Some("Play"));
        let play_content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        play_content.append(&play_icon);
        play_content.append(&play_label);
        let play = gtk::Button::builder().child(&play_content).build();
        play.add_css_class("pill");
        play.add_css_class("filled");
        play.set_margin_end(8);
        let on_play = sender.input_sender().clone();
        play.connect_clicked(move |_| on_play.emit(ArtistAction::Primary));
        actions.append(&play);
        let shuffle = gtk::Button::from_icon_name("media-playlist-shuffle-symbolic");
        shuffle.add_css_class("icon");
        shuffle.add_css_class("on-photo");
        shuffle.set_valign(gtk::Align::Center);
        shuffle.set_tooltip_text(Some("Shuffle play"));
        let on_shuffle = sender.input_sender().clone();
        shuffle.connect_clicked(move |_| on_shuffle.emit(ArtistAction::Shuffle));
        actions.append(&shuffle);
        let follow = gtk::Button::with_label("Follow");
        follow.add_css_class("follow");
        follow.set_valign(gtk::Align::Center);
        actions.append(&follow);
        let more_actions = gtk::Button::from_icon_name("view-more-symbolic");
        more_actions.add_css_class("icon");
        more_actions.add_css_class("on-photo");
        more_actions.set_valign(gtk::Align::Center);
        more_actions.set_tooltip_text(Some("More options"));
        actions.append(&more_actions);
        head.append(&actions);
        sky.append(&head);

        let sheet = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sheet.add_css_class("rise");

        let popular_title = section("Popular");
        popular_title.set_margin_top(4);
        sheet.append(&popular_title);
        let split = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        let tracks = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tracks.set_hexpand(true);
        let popular = gtk::Box::new(gtk::Orientation::Vertical, 2);
        tracks.append(&popular);
        let more = gtk::Button::with_label("Show more");
        more.add_css_class("link");
        more.set_halign(gtk::Align::Start);
        more.set_margin_start(16);
        more.set_margin_top(8);
        more.set_visible(false);
        let on_more = sender.input_sender().clone();
        more.connect_clicked(move |_| on_more.emit(ArtistAction::ToggleMore));
        let links = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        links.set_margin_start(16);
        links.set_margin_top(8);
        more.set_margin_start(0);
        more.set_margin_top(0);
        links.append(&more);
        let songs = gtk::Button::with_label("All songs");
        songs.add_css_class("link");
        let on_songs = sender.input_sender().clone();
        songs.connect_clicked(move |_| {
            on_songs.emit(ArtistAction::OpenDiscography(View::Songs));
        });
        links.append(&songs);
        tracks.append(&links);
        split.append(&tracks);
        let latest = gtk::Box::new(gtk::Orientation::Vertical, 0);
        latest.set_valign(gtk::Align::Start);
        latest.set_margin_end(4);
        latest.set_visible(false);
        split.append(&latest);
        sheet.append(&split);

        let disco_head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        disco_head.set_margin_top(32);
        disco_head.set_margin_start(16);
        disco_head.set_margin_end(16);
        let disco_title = section("Discography");
        disco_title.add_css_class("inline");
        disco_title.set_valign(gtk::Align::Center);
        disco_head.append(&disco_title);
        let filters = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        filters.set_hexpand(true);
        filters.set_halign(gtk::Align::End);
        disco_head.append(&filters);
        let all = gtk::Button::with_label("Show all");
        all.add_css_class("link");
        all.set_valign(gtk::Align::Center);
        let on_all = sender.input_sender().clone();
        all.connect_clicked(move |_| on_all.emit(ArtistAction::ShowAll));
        disco_head.append(&all);
        sheet.append(&disco_head);
        let discography = gtk::Box::new(gtk::Orientation::Vertical, 0);
        discography.set_margin_start(16);
        discography.set_margin_end(16);
        sheet.append(&discography);

        let fans = gtk::Box::new(gtk::Orientation::Vertical, 0);
        fans.set_visible(false);
        fans.append(&section("Fans also like"));
        let related = gtk::FlowBox::new();
        related.set_selection_mode(gtk::SelectionMode::None);
        related.set_homogeneous(true);
        related.set_column_spacing(8);
        related.set_row_spacing(12);
        related.set_min_children_per_line(2);
        related.set_max_children_per_line(10);
        related.set_halign(gtk::Align::Start);
        related.set_margin_start(16);
        related.set_margin_end(16);
        fans.append(&related);
        sheet.append(&fans);

        let about = gtk::Box::new(gtk::Orientation::Horizontal, 28);
        about.add_css_class("about");
        let face_tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        face_tile.add_css_class("card-art");
        face_tile.add_css_class("round");
        face_tile.set_size_request(148, 148);
        face_tile.set_hexpand(false);
        face_tile.set_valign(gtk::Align::Start);
        face_tile.set_overflow(gtk::Overflow::Hidden);
        let face = gtk::Picture::new();
        face.set_content_fit(gtk::ContentFit::Cover);
        let frame = gtk::Overlay::new();
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_size_request(148, 148);
        frame.set_child(Some(&spacer));
        frame.add_overlay(&face);
        face_tile.append(&frame);
        about.append(&face_tile);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text.set_hexpand(true);
        let about_title = section("About");
        about_title.add_css_class("inline");
        about_title.set_margin_bottom(8);
        text.append(&about_title);
        let blurb = gtk::Label::new(None);
        blurb.add_css_class("artist-blurb");
        blurb.set_xalign(0.0);
        blurb.set_wrap(true);
        blurb.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        blurb.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
        blurb.set_max_width_chars(60);
        text.append(&blurb);
        let credit = gtk::Label::new(None);
        credit.add_css_class("credit-line");
        credit.set_xalign(0.0);
        credit.set_margin_top(16);
        text.append(&credit);
        about.append(&text);
        sheet.append(&about);
        column.append(&sheet);

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(false)
            .vexpand(true)
            .child(&column)
            .build();
        root.add_overlay(&scroll);

        let sticky = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        sticky.add_css_class("hero-sticky");
        let small_content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        small_content.append(&gtk::Image::from_icon_name("media-playback-start-symbolic"));
        small_content.append(&gtk::Label::new(Some("Play")));
        let small = gtk::Button::builder().child(&small_content).build();
        small.add_css_class("pill");
        small.add_css_class("filled");
        small.add_css_class("small");
        small.set_valign(gtk::Align::Center);
        let on_small = sender.input_sender().clone();
        small.connect_clicked(move |_| on_small.emit(ArtistAction::Primary));
        sticky.append(&small);
        let sticky_name = gtk::Label::new(None);
        sticky_name.add_css_class("hero-sticky-name");
        sticky_name.set_xalign(0.0);
        sticky_name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        sticky_name.set_hexpand(true);
        sticky_name.set_valign(gtk::Align::Center);
        sticky.append(&sticky_name);
        let reveal = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .transition_duration(200)
            .valign(gtk::Align::Start)
            .child(&sticky)
            .build();
        root.add_overlay(&reveal);

        let adjustment = scroll.vadjustment();
        adjustment.connect_page_size_notify({
            let sky = sky.clone();
            move |adjustment| {
                let hero = (adjustment.page_size() * 0.55) as i32;
                sky.set_size_request(-1, hero.max(HERO_MIN));
            }
        });
        adjustment.connect_value_changed({
            let (tint, shade, sky) = (tint.clone(), shade.clone(), sky.clone());
            move |adjustment| {
                let progress = (adjustment.value() / 420.0).clamp(0.0, 1.0);
                tint.set_opacity(0.6 + 0.4 * progress);
                shade.set_opacity(0.45 * progress);
                reveal.set_reveal_child(adjustment.value() >= sky.height() as f64);
            }
        });

        let (requests, _) = tokio::sync::watch::channel(0);
        let model = ArtistPage {
            services: None,
            listening: false,
            requests,
            artist: ArtistRef {
                uri: String::new(),
                name: String::new(),
            },
            playing: String::new(),
            active_queue: false,
            is_playing: false,
            uris: Vec::new(),
            rows: Vec::new(),
            thumbs: Vec::new(),
            folded: Vec::new(),
            shelves: Vec::new(),
            photo,
            poster,
            tint,
            face,
            name,
            sticky_name,
            detail,
            detail_bone,
            play_icon,
            play_label,
            latest,
            popular,
            more,
            filters,
            discography,
            fans,
            related,
            blurb,
            credit,
            scroll,
        };

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            ArtistAction::Show(services, artist, picture) => {
                self.show(services, artist, picture, &sender)
            }
            ArtistAction::Primary => {
                if self.active_queue {
                    if let Some(services) = &self.services {
                        services.playback.toggle();
                    }
                } else {
                    self.play(0, false);
                }
            }
            ArtistAction::Shuffle => self.play(0, true),
            ArtistAction::PlayTrack(index) => {
                if self.active_queue && self.uris.get(index) == Some(&self.playing) {
                    if let Some(services) = &self.services {
                        services.playback.toggle();
                    }
                } else {
                    self.play(index, false);
                }
            }
            ArtistAction::ToggleMore => {
                let show = self.folded.first().is_some_and(|row| !row.is_visible());
                self.folded.iter().for_each(|row| row.set_visible(show));
                self.more
                    .set_label(if show { "Show less" } else { "Show more" });
            }
            ArtistAction::Filter(index) => {
                for (other, (chip, shelf, _)) in self.shelves.iter().enumerate() {
                    shelf.set_visible(other == index);
                    if other == index {
                        chip.add_css_class("active");
                    } else {
                        chip.remove_css_class("active");
                    }
                }
            }
            ArtistAction::OpenPlaylist(playlist) => {
                let _ = sender.output(ArtistOutput::OpenPlaylist(playlist));
            }
            ArtistAction::OpenArtist(artist, picture) => {
                let _ = sender.output(ArtistOutput::OpenArtist(artist, picture));
            }
            ArtistAction::OpenDiscography(view) => {
                let artist = Box::new(self.artist.clone());
                let _ = sender.output(ArtistOutput::OpenDiscography(artist, view));
            }
            ArtistAction::ShowAll => {
                let group = self
                    .shelves
                    .iter()
                    .find(|(chip, _, _)| chip.has_css_class("active"))
                    .map(|(_, _, group)| *group)
                    .unwrap_or(ReleaseGroup::Albums);
                let artist = Box::new(self.artist.clone());
                let _ = sender.output(ArtistOutput::OpenDiscography(artist, View::Releases(group)));
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
            ArtistCmd::Loaded(request, info) => {
                if request == *self.requests.borrow() {
                    self.loaded(request, *info, &sender);
                }
            }
            ArtistCmd::Portrait(request, path) => {
                if request == *self.requests.borrow() {
                    self.face.set_filename(Some(&path));
                }
            }
            ArtistCmd::Banner(request, path) => {
                if request == *self.requests.borrow() {
                    self.hero(&path);
                    let _ = sender.output(ArtistOutput::Cover(path));
                }
            }
            ArtistCmd::Singles(request, singles) => {
                if request != *self.requests.borrow() {
                    return;
                }
                let Some(position) = self
                    .shelves
                    .iter()
                    .position(|(_, _, group)| *group == ReleaseGroup::Singles)
                else {
                    return;
                };
                let replacement = self.shelf(request, &singles, &sender);
                let old = self.shelves[position].1.clone();
                replacement.set_visible(old.is_visible());
                self.discography
                    .insert_child_after(&replacement, Some(&old));
                self.discography.remove(&old);
                self.shelves[position].1 = replacement.upcast();
            }
            ArtistCmd::Image(request, key, path) => {
                if request != *self.requests.borrow() {
                    return;
                }
                for (thumb_key, picture) in &self.thumbs {
                    if thumb_key == &key {
                        picture.set_filename(Some(&path));
                    }
                }
            }
            ArtistCmd::Playback(event) => self.playback(event),
            ArtistCmd::Failed(error) => tracing::error!("{error}"),
        }
    }

    fn update_view(&self, _: &mut Self::Widgets, _: ComponentSender<Self>) {}
}

impl ArtistPage {
    fn show(
        &mut self,
        services: Arc<Services>,
        artist: ArtistRef,
        picture: Option<String>,
        sender: &ComponentSender<Self>,
    ) {
        let request = (*self.requests.borrow()).wrapping_add(1);
        self.requests.send_replace(request);
        self.artist = artist;
        self.name.set_text(&self.artist.name);
        self.sticky_name.set_text(&self.artist.name);
        self.detail.set_visible(false);
        self.detail_bone.set_visible(true);
        self.poster.set_visible(true);
        self.photo.set_paintable(gtk::gdk::Paintable::NONE);
        self.tint.set_paintable(gtk::gdk::Paintable::NONE);
        self.face.set_paintable(gtk::gdk::Paintable::NONE);
        self.scroll.vadjustment().set_value(0.0);
        self.active_queue = false;
        self.uris.clear();
        self.rows.clear();
        self.thumbs.clear();
        self.folded.clear();
        self.shelves.clear();
        self.more.set_visible(false);
        self.fans.set_visible(false);
        self.blurb.set_text("");
        self.credit.set_text("");
        for container in [
            &self.latest,
            &self.popular,
            &self.filters,
            &self.discography,
        ] {
            while let Some(child) = container.first_child() {
                container.remove(&child);
            }
        }
        while let Some(child) = self.related.first_child() {
            self.related.remove(&child);
        }
        self.refresh_play_button();

        for index in 0..5 {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
            row.add_css_class("track-skeleton");
            let number = bone(14, 10);
            number.set_halign(gtk::Align::End);
            let leading = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            leading.set_size_request(20, -1);
            leading.set_halign(gtk::Align::End);
            leading.append(&number);
            row.append(&leading);
            row.append(&bone(40, 40));
            let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
            text.set_valign(gtk::Align::Center);
            text.set_hexpand(true);
            text.append(&bone(150 + (index % 4) * 30, 14));
            text.append(&bone(90 + (index % 3) * 25, 12));
            row.append(&text);
            row.append(&bone(110, 10));
            row.append(&bone(30, 10));
            self.popular.append(&row);
        }

        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("latest-card");
        let eyebrow = bone(88, 10);
        eyebrow.set_margin_top(2);
        eyebrow.set_margin_bottom(12);
        card.append(&eyebrow);
        let art = bone(196, 196);
        art.add_css_class("tile");
        card.append(&art);
        let name = bone(140, 14);
        name.set_margin_top(12);
        card.append(&name);
        let sub = bone(72, 11);
        sub.set_margin_top(4);
        card.append(&sub);
        self.latest.append(&card);
        self.latest.set_visible(true);

        let grid = gtk::FlowBox::new();
        grid.set_selection_mode(gtk::SelectionMode::None);
        grid.set_homogeneous(true);
        grid.set_column_spacing(8);
        grid.set_row_spacing(12);
        grid.set_min_children_per_line(2);
        grid.set_max_children_per_line(8);
        grid.set_halign(gtk::Align::Start);
        grid.set_margin_top(14);
        for index in 0..6 {
            let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
            body.add_css_class("card");
            let art = bone(160, 160);
            art.add_css_class("tile");
            body.append(&art);
            let name = bone(100 + (index % 3) * 20, 13);
            name.set_margin_top(10);
            body.append(&name);
            let year = bone(36, 11);
            year.set_margin_top(4);
            body.append(&year);
            grid.append(&body);
        }
        self.discography.append(&grid);

        if let Some(picture) = picture {
            self.fetch_picture(request, picture, &services, sender, ArtistCmd::Portrait);
        }

        let session = services.session.clone();
        let uri = self.artist.uri.clone();
        let mut requests = self.requests.subscribe();
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    tokio::select! {
                        result = metadata::artist(&session, &uri) => {
                            let message = match result {
                                Ok(info) => ArtistCmd::Loaded(request, Box::new(info)),
                                Err(error) => ArtistCmd::Failed(format!("artist: {error}")),
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
                            let _ = out.send(ArtistCmd::Playback(event));
                        }
                    })
                    .drop_on_shutdown()
            });
        }

        self.services = Some(services);
    }

    fn fetch_picture(
        &self,
        request: u64,
        picture: String,
        services: &Arc<Services>,
        sender: &ComponentSender<Self>,
        wrap: fn(u64, std::path::PathBuf) -> ArtistCmd,
    ) {
        if let Some(path) = images::cached(&picture) {
            sender.command_sender().emit(wrap(request, path));
            return;
        }
        let session = services.session.clone();
        let mut requests = self.requests.subscribe();
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    tokio::select! {
                        result = images::fetch(&session, &picture) => {
                            if let Ok(path) = result {
                                let _ = out.send(wrap(request, path));
                            }
                        }
                        _ = requests.changed() => {}
                    }
                })
                .drop_on_shutdown()
        });
    }

    fn hero(&self, path: &std::path::Path) {
        self.poster.set_visible(false);
        self.photo.set_filename(Some(path));
        let palette = Palette::from_cover(path);
        if let Some(texture) = duotone(path, palette.tone(0.5, 0.10), palette.tone(0.55, 0.72)) {
            self.tint.set_paintable(Some(&texture));
        }
    }

    fn loaded(&mut self, request: u64, info: ArtistInfo, sender: &ComponentSender<Self>) {
        let Some(services) = self.services.clone() else {
            return;
        };
        if self.face.paintable().is_none()
            && let Some(portrait) = info.portrait_id.clone()
        {
            self.fetch_picture(request, portrait, &services, sender, ArtistCmd::Portrait);
        }
        if let Some(hero) = info.banner.clone().or_else(|| info.portrait_id.clone()) {
            self.fetch_picture(request, hero, &services, sender, ArtistCmd::Banner);
        } else {
            self.poster.set_visible(false);
        }
        self.detail_bone.set_visible(false);
        self.detail.set_visible(true);
        let mut detail = Vec::new();
        if let Some(listeners) = info.monthly_listeners {
            detail.push(format!("{} monthly listeners", text::thousands(listeners)));
        }
        detail.push(text::count(info.albums.len(), "album"));
        detail.push(text::count(info.singles_total, "single"));
        self.detail.set_text(&detail.join(" · "));

        let latest = info
            .albums
            .iter()
            .chain(info.singles.iter())
            .max_by_key(|album| album.year)
            .cloned();
        while let Some(child) = self.latest.first_child() {
            self.latest.remove(&child);
        }
        self.latest.set_visible(latest.is_some());
        if let Some(album) = latest {
            let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let eyebrow = gtk::Label::new(Some("Latest release"));
            eyebrow.add_css_class("latest-eyebrow");
            eyebrow.set_xalign(0.0);
            eyebrow.set_margin_bottom(10);
            body.append(&eyebrow);
            let art = self.art(request, 196, false, album.cover_id.as_deref(), sender);
            art.add_css_class("latest-art");
            body.append(&art);
            let name = gtk::Label::new(Some(&album.name));
            name.add_css_class("latest-name");
            name.set_xalign(0.0);
            name.set_max_width_chars(18);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            name.set_margin_top(10);
            body.append(&name);
            let kind = if info.albums.iter().any(|a| a.uri == album.uri) {
                "Album"
            } else {
                "Single"
            };
            let sub = gtk::Label::new(Some(&format!("{kind} · {}", album.year)));
            sub.add_css_class("card-sub");
            sub.set_xalign(0.0);
            body.append(&sub);
            let card = gtk::Button::builder().child(&body).build();
            card.add_css_class("latest-card");
            card.set_tooltip_text(Some("Open"));
            let entry = self.album_ref(&album);
            let on_open = sender.input_sender().clone();
            card.connect_clicked(move |_| {
                on_open.emit(ArtistAction::OpenPlaylist(Box::new(entry.clone())));
            });
            self.latest.append(&card);
        }

        while let Some(child) = self.popular.first_child() {
            self.popular.remove(&child);
        }
        self.uris = info
            .top_tracks
            .iter()
            .map(|track| track.uri.clone())
            .collect();
        for (index, track) in info.top_tracks.iter().take(10).enumerate() {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);

            let leading = gtk::Overlay::new();
            leading.add_css_class("track-leading");
            leading.set_size_request(20, -1);
            let number = gtk::Label::new(Some(&format!("{:>2}", index + 1)));
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
            let release = info
                .albums
                .iter()
                .chain(info.singles.iter())
                .find(|album| album.cover_id.is_some() && album.cover_id == track.cover_id)
                .map(|album| album.name.as_str())
                .filter(|name| *name != track.name)
                .unwrap_or("Single");
            let release = gtk::Label::new(Some(release));
            release.add_css_class("track-artists");
            release.set_xalign(0.0);
            release.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(&release);
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
            let on_row = sender.input_sender().clone();
            button.connect_clicked(move |_| on_row.emit(ArtistAction::PlayTrack(index)));
            if index >= 5 {
                button.set_visible(false);
                self.folded.push(button.clone().upcast());
            }
            self.rows
                .push((track.uri.clone(), button.clone().upcast(), track_play));
            self.popular.append(&button);
        }
        self.more.set_visible(!self.folded.is_empty());
        self.more.set_label("Show more");

        while let Some(child) = self.discography.first_child() {
            self.discography.remove(&child);
        }
        let groups = [
            (ReleaseGroup::Albums, &info.albums),
            (ReleaseGroup::Singles, &info.singles),
            (ReleaseGroup::Compilations, &info.compilations),
            (ReleaseGroup::AppearsOn, &info.appears_on),
        ];
        for (group, albums) in groups {
            if albums.is_empty() {
                continue;
            }
            let index = self.shelves.len();
            let chip = gtk::Button::with_label(group.title());
            chip.add_css_class("filter");
            if index == 0 {
                chip.add_css_class("active");
            }
            let on_chip = sender.input_sender().clone();
            chip.connect_clicked(move |_| on_chip.emit(ArtistAction::Filter(index)));
            self.filters.append(&chip);

            let shelf = self.shelf(request, albums, sender);
            shelf.set_visible(index == 0);
            self.discography.append(&shelf);
            self.shelves.push((chip, shelf.upcast(), group));
        }
        if info.singles_total > info.singles.len()
            && let Some(services) = self.services.clone()
        {
            let uri = self.artist.uri.clone();
            let mut requests = self.requests.subscribe();
            sender.command(move |out, shutdown| {
                shutdown
                    .register(async move {
                        tokio::select! {
                            result = metadata::discography(&services.session, &uri, ReleaseGroup::Singles) => {
                                if let Ok(singles) = result {
                                    let _ = out.send(ArtistCmd::Singles(request, singles));
                                }
                            }
                            _ = requests.changed() => {}
                        }
                    })
                    .drop_on_shutdown()
            });
        }

        self.fans.set_visible(!info.related.is_empty());
        for artist in info.related.iter().take(GRID_FOLD) {
            let art = self.art(request, 120, true, artist.portrait.as_deref(), sender);
            let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
            body.append(&art);
            let name = gtk::Label::new(Some(&artist.name));
            name.add_css_class("card-name");
            name.set_max_width_chars(12);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            body.append(&name);
            let kind = gtk::Label::new(Some("Artist"));
            kind.add_css_class("card-sub");
            body.append(&kind);
            let card = gtk::Button::builder().child(&body).build();
            card.add_css_class("card");
            let entry = ArtistRef {
                uri: artist.uri.clone(),
                name: artist.name.clone(),
            };
            let portrait = artist.portrait.clone();
            let open = sender.input_sender().clone();
            card.connect_clicked(move |_| {
                open.emit(ArtistAction::OpenArtist(
                    Box::new(entry.clone()),
                    portrait.clone(),
                ));
            });
            self.related.append(&card);
        }

        if let Some(biography) = &info.biography {
            let plain = strip_tags(biography);
            let (body, credit) = plain.rsplit_once(" ~ ").unwrap_or((&plain, ""));
            let first = body.split('\n').next().unwrap_or(body);
            self.blurb.set_text(first);
            self.credit.set_text(&if credit.is_empty() {
                String::new()
            } else {
                format!("Notes by {credit}")
            });
        }

        self.sync_playback();
    }

    fn shelf(
        &mut self,
        request: u64,
        albums: &[AlbumRef],
        sender: &ComponentSender<Self>,
    ) -> gtk::Box {
        let grid = gtk::FlowBox::new();
        grid.set_selection_mode(gtk::SelectionMode::None);
        grid.set_homogeneous(true);
        grid.set_column_spacing(8);
        grid.set_row_spacing(12);
        grid.set_min_children_per_line(2);
        grid.set_max_children_per_line(8);
        grid.set_halign(gtk::Align::Start);
        for album in albums {
            let card = self.card(request, album, sender);
            grid.append(&card);
        }
        let folded: Vec<gtk::FlowBoxChild> = (GRID_FOLD..albums.len())
            .filter_map(|position| grid.child_at_index(position as i32))
            .collect();
        folded.iter().for_each(|child| child.set_visible(false));

        let shelf = gtk::Box::new(gtk::Orientation::Vertical, 0);
        shelf.set_margin_top(14);
        shelf.append(&grid);
        if !folded.is_empty() {
            let all = gtk::Button::with_label("Show all");
            all.add_css_class("link");
            all.set_halign(gtk::Align::Start);
            all.set_margin_top(12);
            all.connect_clicked(move |button| {
                let show = folded.first().is_some_and(|child| !child.is_visible());
                folded.iter().for_each(|child| child.set_visible(show));
                button.set_label(if show { "Show less" } else { "Show all" });
            });
            shelf.append(&all);
        }

        shelf
    }

    fn album_ref(&self, album: &AlbumRef) -> PlaylistRef {
        PlaylistRef {
            uri: album.uri.clone(),
            name: album.name.clone(),
            owner: self.artist.name.clone(),
            length: 0,
            picture: album.cover_id.clone(),
        }
    }

    fn card(
        &mut self,
        request: u64,
        album: &AlbumRef,
        sender: &ComponentSender<Self>,
    ) -> gtk::Button {
        let art = self.art(request, 160, false, album.cover_id.as_deref(), sender);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.append(&art);
        let name = gtk::Label::new(Some(&album.name));
        name.add_css_class("card-name");
        name.set_xalign(0.0);
        name.set_max_width_chars(16);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        body.append(&name);
        let year = gtk::Label::new(Some(&album.year.to_string()));
        year.add_css_class("card-sub");
        year.set_xalign(0.0);
        body.append(&year);

        let card = gtk::Button::builder().child(&body).build();
        card.add_css_class("card");
        let entry = self.album_ref(album);
        let open = sender.input_sender().clone();
        card.connect_clicked(move |_| {
            open.emit(ArtistAction::OpenPlaylist(Box::new(entry.clone())));
        });

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
                        tokio::select! {
                            result = images::fetch(&services.session, &picture) => {
                                if let Ok(path) = result {
                                    let _ = out.send(ArtistCmd::Image(request, picture, path));
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
        self.is_playing = true;
        if let Some(uri) = self.uris.get(index) {
            self.playing.clone_from(uri);
        }
        self.refresh_rows();
        self.refresh_play_button();
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
    }
}

fn duotone(
    path: &std::path::Path,
    dark: (u8, u8, u8),
    light: (u8, u8, u8),
) -> Option<gtk::gdk::MemoryTexture> {
    let texture = gtk::gdk::Texture::from_filename(path).ok()?;
    let (width, height) = (texture.width() as usize, texture.height() as usize);
    let stride = width * 4;
    let mut data = vec![0u8; stride * height];
    texture.download(&mut data, stride);
    for pixel in data.chunks_exact_mut(4) {
        let luma = (0.0722 * pixel[0] as f64 + 0.7152 * pixel[1] as f64 + 0.2126 * pixel[2] as f64)
            / 255.0;
        let mix = |from: u8, to: u8| (from as f64 + (to as f64 - from as f64) * luma).round() as u8;
        pixel[0] = mix(dark.2, light.2);
        pixel[1] = mix(dark.1, light.1);
        pixel[2] = mix(dark.0, light.0);
        pixel[3] = 255;
    }
    Some(gtk::gdk::MemoryTexture::new(
        width as i32,
        height as i32,
        gtk::gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &gtk::glib::Bytes::from_owned(data),
        stride,
    ))
}

fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    for c in text.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

fn bone(width: i32, height: i32) -> gtk::Box {
    let bone = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bone.add_css_class("skeleton");
    bone.set_size_request(width, height);
    bone.set_valign(gtk::Align::Center);
    bone.set_halign(gtk::Align::Start);

    bone
}

fn section(title: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(title));
    label.add_css_class("hero-section");
    label.set_xalign(0.0);

    label
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
