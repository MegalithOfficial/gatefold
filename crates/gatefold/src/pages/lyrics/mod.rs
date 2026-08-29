use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use gatefold_core::{
    lyrics::{self, Lyrics, Provider, Request, Sync},
    player,
    settings::Settings,
};
use relm4::{
    Component, ComponentParts, ComponentSender, RelmWidgetExt,
    adw::{self, prelude::*},
    gtk::{self, gdk, gdk_pixbuf, glib, pango, subclass::prelude::ObjectSubclassIsExt},
};

use crate::{app::Services, palette};

pub const CSS: &str = include_str!("style.css");

const FOLLOW_MS: u32 = 360;
const ANCHOR: f64 = 0.38;
const HOLD: Duration = Duration::from_secs(12);
const BONES: [i32; 7] = [420, 300, 480, 260, 380, 440, 320];
const TONE_SAMPLE: i32 = 48;
const TONES: usize = 4;

pub struct LyricsPage {
    services: Option<Arc<Services>>,
    uri: String,
    state: State,
    candidates: Vec<Lyrics>,
    chosen: usize,
    romanized: bool,
    settings: Settings,
    sheet: Rc<RefCell<Sheet>>,
    states: gtk::Stack,
    backdrop: Backdrop,
    credit: gtk::Label,
    romanize: gtk::ToggleButton,
    sources: gtk::MenuButton,
    source_rows: gtk::Box,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Loading,
    Sheet,
    Missing,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Idle | State::Missing => "empty",
            State::Loading => "bones",
            State::Sheet => "sheet",
        }
    }
}

pub enum LyricsAction {
    SetServices(Arc<Services>),
    Cover(PathBuf),
    Pick(usize, Option<usize>),
    Romanize(bool),
    Source(usize),
    Resync,
    Backdrop(bool),
}

impl std::fmt::Debug for LyricsAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LyricsAction::SetServices(_) => write!(formatter, "SetServices"),
            LyricsAction::Cover(path) => write!(formatter, "Cover({})", path.display()),
            LyricsAction::Pick(line, word) => write!(formatter, "Pick({line}, {word:?})"),
            LyricsAction::Romanize(on) => write!(formatter, "Romanize({on})"),
            LyricsAction::Source(index) => write!(formatter, "Source({index})"),
            LyricsAction::Resync => write!(formatter, "Resync"),
            LyricsAction::Backdrop(on) => write!(formatter, "Backdrop({on})"),
        }
    }
}

#[derive(Debug)]
pub enum LyricsUpdate {
    Playback(player::Event),
    Loaded(String, Vec<Lyrics>),
}

#[relm4::component(pub)]
impl Component for LyricsPage {
    type Init = ();
    type Input = LyricsAction;
    type Output = ();
    type CommandOutput = LyricsUpdate;

    view! {
        gtk::Overlay {
            set_overflow: gtk::Overflow::Hidden,
            add_css_class: "lyrics-page",

            #[local_ref]
            backdrop -> Backdrop {
                #[watch]
                set_visible: model.settings.lyrics_backdrop,
            },

            add_overlay = &gtk::Box {
                #[watch]
                set_visible: model.state != State::Idle && model.settings.lyrics_backdrop,
                add_css_class: "lyrics-scrim",
            },

            #[local_ref]
            add_overlay = states -> gtk::Stack {
                set_transition_type: gtk::StackTransitionType::Crossfade,
                set_transition_duration: 260,
                #[watch]
                set_visible_child_name: model.state.name(),
            },

            add_overlay = &gtk::Box {
                set_halign: gtk::Align::End,
                set_valign: gtk::Align::Start,
                set_spacing: 4,
                add_css_class: "lyrics-tools",

                gtk::ToggleButton {
                    set_icon_name: "gatefold-palette-symbolic",
                    set_tooltip_text: Some("Cover colours"),
                    set_focus_on_click: false,
                    set_active: model.settings.lyrics_backdrop,
                    add_css_class: "icon",
                    #[watch]
                    set_visible: model.state != State::Idle,
                    connect_toggled[sender] => move |button| {
                        sender.input(LyricsAction::Backdrop(button.is_active()));
                    },
                },

                #[local_ref]
                romanize -> gtk::ToggleButton {
                    set_icon_name: "gatefold-romanize-symbolic",
                    set_tooltip_text: Some("Romanize"),
                    set_focus_on_click: false,
                    add_css_class: "icon",
                    #[watch]
                    set_visible: model.current().is_some_and(|lyrics| lyrics.romanization_available),
                    connect_toggled[sender] => move |button| {
                        sender.input(LyricsAction::Romanize(button.is_active()));
                    },
                },

                #[local_ref]
                sources -> gtk::MenuButton {
                    set_icon_name: "gatefold-sources-symbolic",
                    set_tooltip_text: Some("Lyrics source"),
                    set_focus_on_click: false,
                    #[watch]
                    set_visible: matches!(model.state, State::Sheet | State::Missing),
                },
            },

            #[local_ref]
            add_overlay = resync -> gtk::Button {
                set_halign: gtk::Align::End,
                set_valign: gtk::Align::End,
                set_tooltip_text: Some("Back to the current line"),
                set_focus_on_click: false,
                set_can_target: false,
                add_css_class: "icon",
                add_css_class: "lyrics-resync",
                connect_clicked => LyricsAction::Resync,
            },
        }
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let credit = gtk::Label::new(None);
        credit.set_xalign(0.0);
        credit.add_css_class("credit-line");
        credit.add_css_class("lyrics-credit");
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.add_css_class("lyrics-sheet");
        column.append(&body);
        column.append(&credit);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_child(Some(&column));

        let sheet = Rc::new(RefCell::new(Sheet {
            lyrics: None,
            lines: Vec::new(),
            body: body.clone(),
            scroll: scroll.clone(),
            anchor: (0, Instant::now()),
            playing: false,
            active: None,
            hold: None,
            settle: None,
            motion: None,
            resync: gtk::Button::from_icon_name("gatefold-down-symbolic"),
        }));
        body.add_tick_callback({
            let sheet = sheet.clone();
            move |body, _| {
                if body.is_mapped() {
                    sheet.borrow_mut().frame();
                }
                glib::ControlFlow::Continue
            }
        });
        body.connect_map({
            let sheet = sheet.clone();
            move |_| sheet.borrow_mut().settle = Some(false)
        });
        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        wheel.connect_scroll({
            let sheet = sheet.clone();
            move |_, _, _| {
                sheet.borrow_mut().hold();
                glib::Propagation::Proceed
            }
        });
        scroll.add_controller(wheel);

        let bones = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bones.add_css_class("lyrics-bones");
        for width in BONES {
            let bone = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            bone.add_css_class("skeleton");
            bone.add_css_class("lyrics-bone");
            bone.set_halign(gtk::Align::Start);
            bone.set_size_request(width, 0);
            bones.append(&bone);
        }

        let empty = gtk::Label::new(Some("Play something to see its lyrics"));
        empty.add_css_class("lyrics-empty");

        let source_rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let source_head = gtk::Label::new(Some("Lyrics source"));
        source_head.set_xalign(0.0);
        source_head.add_css_class("lyrics-source-head");
        let source_sheet = gtk::Box::new(gtk::Orientation::Vertical, 0);
        source_sheet.set_width_request(220);
        source_sheet.append(&source_head);
        source_sheet.append(&source_rows);
        let menu = gtk::Popover::new();
        menu.set_has_arrow(false);
        menu.set_offset(0, 6);
        menu.remove_css_class("background");
        menu.add_css_class("quick-menu");
        menu.set_child(Some(&source_sheet));
        let sources = gtk::MenuButton::new();
        sources.set_popover(Some(&menu));
        sources.set_direction(gtk::ArrowType::Down);

        let model = LyricsPage {
            services: None,
            uri: String::new(),
            state: State::Idle,
            candidates: Vec::new(),
            chosen: 0,
            romanized: false,
            settings: Settings::load(),
            sheet,
            states: gtk::Stack::new(),
            backdrop: Backdrop::new(),
            credit,
            romanize: gtk::ToggleButton::new(),
            sources,
            source_rows,
        };
        let states = &model.states;
        states.add_named(&scroll, Some("sheet"));
        states.add_named(&bones, Some("bones"));
        states.add_named(&empty, Some("empty"));
        let backdrop = &model.backdrop;
        let romanize = &model.romanize;
        let sources = &model.sources;
        let resync = &model.sheet.borrow().resync.clone();
        let widgets = view_output!();
        let _ = root;
        ComponentParts { model, widgets }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            LyricsAction::SetServices(services) => {
                let mut events = services.playback.events();
                self.services = Some(services);
                sender.command(|out, shutdown| {
                    shutdown
                        .register(async move {
                            while let Ok(event) = events.recv().await {
                                let _ = out.send(LyricsUpdate::Playback(event));
                            }
                        })
                        .drop_on_shutdown()
                });
            }
            LyricsAction::Cover(path) => self.backdrop.set_tones(tones(&path)),
            LyricsAction::Pick(line, word) => {
                let position_ms = self
                    .sheet
                    .borrow()
                    .lyrics
                    .as_ref()
                    .and_then(|lyrics| lyrics.seek_position(line, word));
                if let (Some(services), Some(position_ms)) = (&self.services, position_ms) {
                    services.playback.seek(position_ms);
                    self.sheet.borrow_mut().mark(position_ms);
                }
            }
            LyricsAction::Romanize(on) => {
                if self.romanized != on {
                    self.romanized = on;
                    self.show(&sender);
                }
            }
            LyricsAction::Resync => self.sheet.borrow_mut().resync(),
            LyricsAction::Backdrop(on) => {
                self.settings.lyrics_backdrop = on;
                self.settings.save();
            }
            LyricsAction::Source(index) => {
                if index < self.candidates.len() && index != self.chosen {
                    self.chosen = index;
                    self.romanized = false;
                    self.romanize.set_active(false);
                    self.menu(&sender);
                    self.show(&sender);
                }
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
            LyricsUpdate::Playback(event) => match event {
                player::Event::TrackChanged {
                    uri,
                    name,
                    artists,
                    duration_ms,
                    ..
                } => {
                    self.uri = uri.clone();
                    self.state = State::Loading;
                    self.candidates.clear();
                    self.chosen = 0;
                    self.romanized = false;
                    self.romanize.set_active(false);
                    self.sheet.borrow_mut().load(None, &sender);
                    let Some(services) = self.services.clone() else {
                        return;
                    };
                    let request = Request {
                        uri: uri.clone(),
                        title: name,
                        artists: artists.into_iter().map(|artist| artist.name).collect(),
                        duration_ms,
                    };
                    sender.oneshot_command(async move {
                        let found = lyrics::fetch_all(&services.session(), &request).await;
                        LyricsUpdate::Loaded(uri, found)
                    });
                }
                player::Event::Playing { uri, position_ms } => {
                    let mut sheet = self.sheet.borrow_mut();
                    sheet.playing = true;
                    if uri == self.uri {
                        sheet.mark(position_ms);
                    }
                }
                player::Event::Paused { uri, position_ms } => {
                    let mut sheet = self.sheet.borrow_mut();
                    sheet.playing = false;
                    if uri == self.uri {
                        sheet.mark(position_ms);
                    }
                }
                player::Event::Position { uri, position_ms } => {
                    if uri == self.uri {
                        self.sheet.borrow_mut().mark(position_ms);
                    }
                }
                player::Event::Stopped => self.sheet.borrow_mut().playing = false,
                _ => {}
            },
            LyricsUpdate::Loaded(uri, found) => {
                if uri != self.uri {
                    return;
                }
                self.chosen = found
                    .iter()
                    .enumerate()
                    .rev()
                    .max_by_key(|(_, lyrics)| lyrics.sync)
                    .map_or(0, |(index, _)| index);
                self.candidates = found;
                self.state = if self.candidates.is_empty() {
                    State::Missing
                } else {
                    State::Sheet
                };
                self.menu(&sender);
                self.show(&sender);
            }
        }
        let empty = self
            .states
            .child_by_name("empty")
            .and_downcast::<gtk::Label>()
            .expect("empty label");
        empty.set_label(match self.state {
            State::Missing => "No lyrics for this song",
            _ => "Play something to see its lyrics",
        });
    }
}

impl LyricsPage {
    fn current(&self) -> Option<&Lyrics> {
        self.candidates.get(self.chosen)
    }

    fn menu(&self, sender: &ComponentSender<Self>) {
        while let Some(row) = self.source_rows.first_child() {
            self.source_rows.remove(&row);
        }
        let popover = self.sources.popover();
        for provider in [Provider::Spotify, Provider::Amll, Provider::Lrclib] {
            let found = self
                .candidates
                .iter()
                .position(|candidate| candidate.source == provider);
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            let name = gtk::Label::new(Some(
                found.map_or(provider.name(), |index| &self.candidates[index].attribution),
            ));
            name.set_xalign(0.0);
            name.set_hexpand(true);
            name.add_css_class("quick-name");
            content.append(&name);
            let note =
                gtk::Label::new(Some(match found.map(|index| self.candidates[index].sync) {
                    Some(Sync::Word) => "Word sync",
                    Some(Sync::Line) => "Line sync",
                    Some(Sync::Unsynced) => "Plain",
                    None => "Unavailable",
                }));
            note.add_css_class("menu-note");
            content.append(&note);
            let check = gtk::Image::from_icon_name("gatefold-check-symbolic");
            check.add_css_class("menu-check");
            check.set_opacity(if found == Some(self.chosen) { 1.0 } else { 0.0 });
            content.append(&check);
            let row = gtk::Button::builder().child(&content).build();
            row.add_css_class("quick-row");
            row.set_focus_on_click(false);
            row.set_sensitive(found.is_some());
            if let Some(index) = found {
                row.connect_clicked({
                    let sender = sender.clone();
                    let popover = popover.clone();
                    move |_| {
                        if let Some(popover) = &popover {
                            popover.popdown();
                        }
                        sender.input(LyricsAction::Source(index));
                    }
                });
            }
            self.source_rows.append(&row);
        }
    }

    fn show(&mut self, sender: &ComponentSender<Self>) {
        let Some(chosen) = self.current() else {
            self.sheet.borrow_mut().load(None, sender);
            return;
        };
        let lyrics = self
            .romanized
            .then(|| chosen.romanized())
            .flatten()
            .unwrap_or_else(|| chosen.clone());
        self.credit
            .set_label(&format!("Lyrics by {}", lyrics.attribution));
        self.sheet.borrow_mut().load(Some(lyrics), sender);
    }
}

fn tones(path: &std::path::Path) -> Vec<gdk::RGBA> {
    let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_file_at_scale(path, TONE_SAMPLE, TONE_SAMPLE, false)
    else {
        return Vec::new();
    };
    let bytes = pixbuf.read_pixel_bytes();
    let channels = pixbuf.n_channels() as usize;
    let stride = pixbuf.rowstride() as usize;
    let mut bins: HashMap<(u8, u8, u8), (usize, f64, f64, f64)> = HashMap::new();
    for y in 0..pixbuf.height() as usize {
        for x in 0..pixbuf.width() as usize {
            let i = y * stride + x * channels;
            let (hue, saturation, value) = palette::to_hsv(
                bytes[i] as f64 / 255.0,
                bytes[i + 1] as f64 / 255.0,
                bytes[i + 2] as f64 / 255.0,
            );
            if value < 0.12 {
                continue;
            }
            let key = (
                (hue / 30.0) as u8,
                (saturation * 3.0).min(2.0) as u8,
                (value * 3.0).min(2.0) as u8,
            );
            let bin = bins.entry(key).or_default();
            bin.0 += 1;
            bin.1 += hue;
            bin.2 += saturation;
            bin.3 += value;
        }
    }
    let mut ranked: Vec<(usize, f64, f64, f64)> = bins
        .into_values()
        .map(|(count, hue, saturation, value)| {
            let n = count as f64;
            (count, hue / n, saturation / n, value / n)
        })
        .collect();
    ranked.sort_by(|a, b| {
        let weight = |bin: &(usize, f64, f64, f64)| bin.0 as f64 * (0.4 + bin.2);
        weight(b).total_cmp(&weight(a))
    });
    let mut picked: Vec<(f64, f64, f64)> = Vec::new();
    for (_, hue, saturation, value) in ranked {
        let distinct = picked.iter().all(|(h, s, _)| {
            let apart = (hue - h).abs().min(360.0 - (hue - h).abs());
            apart > 28.0 || (saturation - s).abs() > 0.35
        });
        if distinct {
            picked.push((hue, saturation, value));
        }
        if picked.len() == TONES {
            break;
        }
    }
    picked
        .into_iter()
        .map(|(hue, saturation, value)| {
            let (r, g, b) = palette::to_rgb(
                hue,
                (saturation * 1.25).clamp(0.0, 0.9),
                value.clamp(0.4, 0.74),
            );
            gdk::RGBA::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
        })
        .collect()
}

mod backdrop {
    use std::cell::{Cell, RefCell};

    use relm4::gtk::{self, gdk, glib, graphene, gsk, prelude::*, subclass::prelude::*};

    const BLUR: f64 = 48.0;
    const DRIFTS: [(f32, f32, f32, f32); 4] = [
        (0.22, 0.18, 0.031, 0.024),
        (0.74, 0.26, 0.019, 0.037),
        (0.30, 0.78, 0.027, 0.021),
        (0.80, 0.80, 0.023, 0.029),
    ];

    #[derive(Default)]
    pub struct Backdrop {
        pub tones: RefCell<Vec<gdk::RGBA>>,
        pub time: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Backdrop {
        const NAME: &'static str = "GatefoldBackdrop";
        type Type = super::Backdrop;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Backdrop {}

    impl WidgetImpl for Backdrop {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let tones = self.tones.borrow();
            if tones.is_empty() {
                return;
            }
            let widget = self.obj();
            let (width, height) = (widget.width() as f32, widget.height() as f32);
            let bounds = graphene::Rect::new(0.0, 0.0, width, height);
            let reach = width.max(height);
            let t = self.time.get() as f32;
            let mut deep = tones[0];
            deep.set_red(deep.red() * 0.28);
            deep.set_green(deep.green() * 0.28);
            deep.set_blue(deep.blue() * 0.28);
            snapshot.append_color(&deep, &bounds);
            snapshot.push_blur(BLUR);
            for (index, tone) in tones.iter().enumerate() {
                let (cx, cy, fx, fy) = DRIFTS[index % DRIFTS.len()];
                let phase = index as f32 * 1.7;
                let x = width * (cx + 0.22 * (t * fx * std::f32::consts::TAU + phase).sin());
                let y = height * (cy + 0.22 * (t * fy * std::f32::consts::TAU + phase * 0.6).cos());
                let radius =
                    reach * (0.42 + 0.06 * (t * 0.017 * std::f32::consts::TAU + phase).sin());
                let mut clear = *tone;
                clear.set_alpha(0.0);
                snapshot.append_radial_gradient(
                    &bounds,
                    &graphene::Point::new(x, y),
                    radius,
                    radius,
                    0.0,
                    1.0,
                    &[
                        gsk::ColorStop::new(0.0, *tone),
                        gsk::ColorStop::new(0.55, tone.with_alpha(0.45)),
                        gsk::ColorStop::new(1.0, clear),
                    ],
                );
            }
            snapshot.pop();
        }
    }

    trait Faded {
        fn with_alpha(&self, alpha: f32) -> gdk::RGBA;
    }

    impl Faded for gdk::RGBA {
        fn with_alpha(&self, alpha: f32) -> gdk::RGBA {
            gdk::RGBA::new(self.red(), self.green(), self.blue(), alpha)
        }
    }
}

glib::wrapper! {
    pub struct Backdrop(ObjectSubclass<backdrop::Backdrop>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Backdrop {
    fn new() -> Self {
        let backdrop: Self = glib::Object::new();
        backdrop.set_hexpand(true);
        backdrop.set_vexpand(true);
        backdrop.add_tick_callback(move |backdrop, clock| {
            if backdrop.is_mapped() && !backdrop.imp().tones.borrow().is_empty() {
                backdrop
                    .imp()
                    .time
                    .set(clock.frame_time() as f64 / 1_000_000.0);
                backdrop.queue_draw();
            }
            glib::ControlFlow::Continue
        });

        backdrop
    }

    fn set_tones(&self, tones: Vec<gdk::RGBA>) {
        self.imp().tones.replace(tones);
        self.queue_draw();
    }
}

struct Sheet {
    lyrics: Option<Lyrics>,
    lines: Vec<Lyric>,
    body: gtk::Box,
    scroll: gtk::ScrolledWindow,
    anchor: (u32, Instant),
    playing: bool,
    active: Option<usize>,
    hold: Option<Instant>,
    settle: Option<bool>,
    motion: Option<adw::TimedAnimation>,
    resync: gtk::Button,
}

impl Sheet {
    fn position(&self) -> u32 {
        let (position_ms, since) = self.anchor;
        if self.playing {
            position_ms.saturating_add(since.elapsed().as_millis() as u32)
        } else {
            position_ms
        }
    }

    fn mark(&mut self, position_ms: u32) {
        self.anchor = (position_ms, Instant::now());
    }

    fn load(&mut self, lyrics: Option<Lyrics>, sender: &ComponentSender<LyricsPage>) {
        for line in self.lines.drain(..) {
            line.stop();
            self.body.remove(&line);
        }
        self.active = None;
        self.hold = None;
        self.settle = Some(false);
        if let Some(motion) = self.motion.take() {
            motion.pause();
        }
        self.scroll.vadjustment().set_value(0.0);

        let Some(column) = self.body.parent() else {
            return;
        };
        column.set_class_active(
            "plain",
            lyrics
                .as_ref()
                .is_some_and(|lyrics| lyrics.sync == Sync::Unsynced),
        );
        if let Some(lyrics) = &lyrics {
            for (index, line) in lyrics.lines.iter().enumerate() {
                let lyric = Lyric::new(line, lyrics.sync == Sync::Word);
                if line.start_ms.is_some() || !line.words.is_empty() {
                    let click = gtk::GestureClick::new();
                    click.connect_released({
                        let sender = sender.clone();
                        let lyric = lyric.clone();
                        move |_, _, x, y| {
                            sender.input(LyricsAction::Pick(index, lyric.word_at(x, y)));
                        }
                    });
                    lyric.add_controller(click);
                }
                self.body.append(&lyric);
                self.lines.push(lyric);
            }
        }
        self.lyrics = lyrics;
    }

    fn frame(&mut self) {
        let Some(lyrics) = &self.lyrics else {
            return;
        };
        if lyrics.sync == Sync::Unsynced {
            return;
        }
        let position = self.position();
        let line = lyrics.active_line(position);
        if line != self.active {
            if let Some(previous) = self.active.and_then(|index| self.lines.get(index)) {
                previous.dim();
            }
            if let Some(current) = line.and_then(|index| self.lines.get(index)) {
                current.light();
                if self.hold.is_none() {
                    self.settle.get_or_insert(true);
                }
            }
            self.active = line;
        }
        if let Some(index) = line
            && lyrics.sync == Sync::Word
        {
            self.lines[index].set_position(position);
        }
        if self.hold.is_some_and(|since| since.elapsed() > HOLD) {
            self.resync();
        }
        let away = self.hold.is_some() && self.active.is_some();
        self.resync.set_class_active("shown", away);
        self.resync.set_can_target(away);
        if let Some(line) = away
            .then_some(self.active)
            .flatten()
            .and_then(|index| self.lines.get(index))
            && let Some(bounds) = line.compute_bounds(&self.scroll)
        {
            let below = bounds.y() as f64 > self.scroll.vadjustment().page_size() * ANCHOR;
            self.resync.set_icon_name(if below {
                "gatefold-down-symbolic"
            } else {
                "gatefold-up-symbolic"
            });
        }
        if let Some(animate) = self.settle
            && self.follow(animate)
        {
            self.settle = None;
        }
    }

    fn follow(&mut self, animate: bool) -> bool {
        let Some(line) = self.active.and_then(|index| self.lines.get(index)) else {
            return true;
        };
        if line.height() == 0 {
            return false;
        }
        let Some(bounds) = line.compute_bounds(&self.scroll) else {
            return false;
        };
        let adjustment = self.scroll.vadjustment();
        let page = adjustment.page_size();
        let target = (adjustment.value() + bounds.y() as f64 + bounds.height() as f64 / 2.0
            - page * ANCHOR)
            .clamp(
                adjustment.lower(),
                (adjustment.upper() - page).max(adjustment.lower()),
            );
        if let Some(motion) = self.motion.take() {
            motion.pause();
        }
        if !animate {
            adjustment.set_value(target);
            return true;
        }
        let motion = adw::TimedAnimation::new(
            &self.scroll,
            adjustment.value(),
            target,
            FOLLOW_MS,
            adw::PropertyAnimationTarget::new(&adjustment, "value"),
        );
        motion.set_easing(adw::Easing::EaseOutCubic);
        motion.play();
        self.motion = Some(motion);
        true
    }

    fn resync(&mut self) {
        self.hold = None;
        self.settle = Some(true);
    }

    fn hold(&mut self) {
        self.hold = Some(Instant::now());
        self.settle = None;
        if let Some(motion) = self.motion.take() {
            motion.pause();
        }
    }
}

mod lyric {
    use std::cell::{Cell, RefCell};

    use relm4::{
        adw,
        gtk::{self, glib, graphene, pango, prelude::*, subclass::prelude::*},
    };

    const MEASURE: i32 = 736;
    const MIN_WIDTH: i32 = 160;
    const RISE: f32 = 0.04;
    const BASE_LIFT: f64 = 0.3;

    pub struct Span {
        pub start: usize,
        pub end: usize,
        pub from_ms: u32,
        pub to_ms: u32,
    }

    #[derive(Default)]
    pub struct Lyric {
        pub text: RefCell<String>,
        pub spans: RefCell<Vec<Span>>,
        pub lit: Cell<f64>,
        pub position_ms: Cell<u32>,
        pub fade: RefCell<Option<adw::TimedAnimation>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Lyric {
        const NAME: &'static str = "GatefoldLyric";
        type Type = super::Lyric;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Lyric {}

    impl WidgetImpl for Lyric {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let layout = self.layout(if orientation == gtk::Orientation::Vertical {
                for_size
            } else {
                -1
            });
            let (_, logical) = layout.pixel_extents();
            match orientation {
                gtk::Orientation::Vertical => (logical.height(), logical.height(), -1, -1),
                _ => (
                    MIN_WIDTH.min(logical.width()),
                    logical.width().min(MEASURE),
                    -1,
                    -1,
                ),
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let layout = self.layout(widget.width());
            let lit = self.lit.get();
            let words = !self.spans.borrow().is_empty();
            let base = widget.color();
            let lift = if words { lit * BASE_LIFT } else { lit };
            let mut ink = base;
            ink.set_alpha(base.alpha() + (1.0 - base.alpha()) * lift as f32);
            let mut on = base;
            on.set_alpha(base.alpha() + (1.0 - base.alpha()) * lit as f32);

            let mid = widget.height() as f32 / 2.0;
            let rise = self.rise();
            snapshot.save();
            snapshot.translate(&graphene::Point::new(0.0, mid));
            snapshot.scale(rise, rise);
            snapshot.translate(&graphene::Point::new(0.0, -mid));
            snapshot.append_layout(&layout, &ink);
            if words && lit > 0.0 {
                for rect in self.fill(&layout) {
                    snapshot.push_clip(&rect);
                    snapshot.append_layout(&layout, &on);
                    snapshot.pop();
                }
            }
            snapshot.restore();
        }
    }

    impl Lyric {
        pub fn layout(&self, width: i32) -> pango::Layout {
            let layout = self.obj().create_pango_layout(Some(&self.text.borrow()));
            layout.set_wrap(pango::WrapMode::WordChar);
            if width > 0 {
                layout.set_width(width.min(MEASURE) * pango::SCALE);
            }
            layout
        }

        pub fn rise(&self) -> f32 {
            1.0 + RISE * self.lit.get() as f32
        }

        fn fill(&self, layout: &pango::Layout) -> Vec<graphene::Rect> {
            let position = self.position_ms.get();
            let spans = self.spans.borrow();
            let scale = pango::SCALE as f32;
            let mut rects = Vec::new();
            let mut iter = layout.iter();
            loop {
                let Some(line) = iter.line_readonly() else {
                    break;
                };
                let (_, logical) = iter.line_extents();
                let start = line.start_index() as usize;
                let end = start + line.length() as usize;
                for span in spans
                    .iter()
                    .filter(|span| span.start < end && span.end > start)
                {
                    let done = if position >= span.to_ms {
                        1.0
                    } else if position <= span.from_ms {
                        continue;
                    } else {
                        (position - span.from_ms) as f32 / (span.to_ms - span.from_ms) as f32
                    };
                    let a = line.index_to_x(span.start.max(start) as i32, false) as f32 / scale;
                    let b = line.index_to_x(span.end.min(end) as i32, false) as f32 / scale;
                    let width = (a - b).abs() * done;
                    let x = if a <= b { a } else { a - width };
                    rects.push(graphene::Rect::new(
                        logical.x() as f32 / scale + x,
                        logical.y() as f32 / scale,
                        width,
                        logical.height() as f32 / scale,
                    ));
                }
                if !iter.next_line() {
                    break;
                }
            }
            rects
        }
    }
}

glib::wrapper! {
    pub struct Lyric(ObjectSubclass<lyric::Lyric>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Lyric {
    fn new(line: &lyrics::Line, by_word: bool) -> Self {
        let lyric: Self = glib::Object::new();
        let mut text = String::new();
        let spans = line
            .words
            .iter()
            .filter(|_| by_word)
            .map(|word| {
                let start = text.len();
                text.push_str(&word.text);
                lyric::Span {
                    start,
                    end: text.len(),
                    from_ms: word.start_ms,
                    to_ms: word.end_ms,
                }
            })
            .collect::<Vec<_>>();
        if spans.is_empty() {
            text = line.text.clone();
        }
        lyric.imp().text.replace(text);
        lyric.imp().spans.replace(spans);
        lyric.add_css_class("lyric");
        if line.start_ms.is_some() || !line.words.is_empty() {
            lyric.add_css_class("synced");
            lyric.set_cursor_from_name(Some("pointer"));
        }

        lyric
    }

    fn word_at(&self, x: f64, y: f64) -> Option<usize> {
        let imp = self.imp();
        let rise = imp.rise() as f64;
        let mid = self.height() as f64 / 2.0;
        let layout = imp.layout(self.width());
        let (inside, index, _) = layout.xy_to_index(
            (x / rise * pango::SCALE as f64) as i32,
            (((y - mid) / rise + mid) * pango::SCALE as f64) as i32,
        );
        if !inside {
            return None;
        }
        let index = index as usize;
        imp.spans
            .borrow()
            .iter()
            .position(|span| span.start <= index && index < span.end)
    }

    fn light(&self) {
        self.fade(1.0, 320, adw::Easing::EaseOutCubic);
    }

    fn dim(&self) {
        self.fade(0.0, 260, adw::Easing::EaseInCubic);
    }

    fn stop(&self) {
        if let Some(fade) = self.imp().fade.take() {
            fade.pause();
        }
    }

    fn fade(&self, to: f64, duration: u32, easing: adw::Easing) {
        self.stop();
        let lyric = self.clone();
        let target = adw::CallbackAnimationTarget::new(move |value| {
            lyric.imp().lit.set(value);
            lyric.queue_draw();
        });
        let fade = adw::TimedAnimation::new(self, self.imp().lit.get(), to, duration, target);
        fade.set_easing(easing);
        fade.play();
        self.imp().fade.replace(Some(fade));
    }

    fn set_position(&self, position_ms: u32) {
        if self.imp().position_ms.replace(position_ms) != position_ms {
            self.queue_draw();
        }
    }
}
