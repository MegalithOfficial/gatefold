use std::{
    cell::{Cell, RefCell},
    f64::consts::PI,
    rc::Rc,
};

use relm4::{
    Component, ComponentParts, ComponentSender,
    adw::{self, prelude::*},
    gtk::{self, cairo, gdk, glib, subclass::prelude::*},
};

use crate::palette::Palette;

pub const CSS: &str = include_str!("style.css");

const RPM: f64 = 33.3;
const IDLE_RPM: f64 = 6.0;
const SPIN_UP: f64 = 0.7;
const SPIN_DOWN: f64 = 0.35;
const ARM_REST: f64 = 90.0;
const ARM_PLAY: f64 = 98.0;

pub fn palette() -> Palette {
    Palette {
        hue: 32.0,
        saturation: 0.6,
    }
}

pub struct Welcome {
    turntable: Turntable,
    sign_in: gtk::Button,
    sign_in_label: gtk::Label,
    cancel: gtk::Button,
    note: gtk::Label,
}

#[derive(Debug)]
pub enum WelcomeAction {
    SignIn,
    Cancel,
    Back,
    Idle,
    Failed(String),
}

#[derive(Debug)]
pub enum WelcomeOutput {
    SignIn,
    Cancel,
    Back,
}

#[relm4::component(pub)]
impl Component for Welcome {
    type Init = ();
    type Input = WelcomeAction;
    type Output = WelcomeOutput;
    type CommandOutput = ();

    view! {
        gtk::Overlay {
            add_css_class: "welcome",

            #[wrap(Some)]
            #[local_ref]
            set_child = platter -> Platter {},

            add_overlay = &gtk::Label {
                set_label: "Gatefold",
                set_halign: gtk::Align::Start,
                set_valign: gtk::Align::Start,
                add_css_class: "welcome-mark",
            },

            add_overlay = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_halign: gtk::Align::End,
                set_valign: gtk::Align::Center,
                set_width_request: 340,
                add_css_class: "welcome-copy",

                gtk::Label {
                    set_label: "Put a record on.",
                    set_xalign: 0.0,
                    add_css_class: "welcome-title",
                },

                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 16,
                    set_margin_top: 20,

                    #[local_ref]
                    sign_in -> gtk::Button {
                        add_css_class: "pill",
                        add_css_class: "filled",
                        set_halign: gtk::Align::Start,
                        connect_clicked => WelcomeAction::SignIn,
                    },

                    #[local_ref]
                    cancel -> gtk::Button {
                        set_label: "Cancel",
                        add_css_class: "link",
                        set_valign: gtk::Align::Center,
                        set_visible: false,
                        connect_clicked => WelcomeAction::Cancel,
                    },
                },

                #[local_ref]
                note -> gtk::Label {
                    set_xalign: 0.0,
                    set_wrap: true,
                    set_margin_top: 12,
                    add_css_class: "welcome-note",
                },
            },

            add_overlay = &gtk::WindowControls {
                set_side: gtk::PackType::End,
                set_halign: gtk::Align::End,
                set_valign: gtk::Align::Start,
                set_margin_top: 12,
                set_margin_end: 12,
                add_css_class: "floating",
            },
        }
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let turntable = Turntable::new(palette());
        let platter = &turntable.platter;
        let sign_in_label = gtk::Label::new(Some("Sign in with Spotify"));
        let sign_in = gtk::Button::builder().child(&sign_in_label).build();
        let cancel = gtk::Button::new();
        let note = gtk::Label::new(Some("Needs Spotify Premium."));
        let widgets = view_output!();

        let escape = gtk::EventControllerKey::new();
        escape.connect_key_pressed({
            let sender = sender.clone();
            move |_, key, _, _| {
                if key == gtk::gdk::Key::Escape {
                    sender.input(WelcomeAction::Back);
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        });
        root.add_controller(escape);

        let model = Welcome {
            turntable,
            sign_in,
            sign_in_label,
            cancel,
            note,
        };

        ComponentParts { model, widgets }
    }

    fn update(&mut self, action: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match action {
            WelcomeAction::SignIn => {
                self.sign_in.set_sensitive(false);
                self.sign_in_label.set_text("Waiting for Spotify…");
                self.cancel.set_visible(true);
                self.note.set_text("Finish signing in in your browser.");
                self.turntable.play();
                let _ = sender.output(WelcomeOutput::SignIn);
            }
            WelcomeAction::Cancel => {
                self.idle("Needs Spotify Premium.");
                let _ = sender.output(WelcomeOutput::Cancel);
            }
            WelcomeAction::Back => {
                let _ = sender.output(WelcomeOutput::Back);
            }
            WelcomeAction::Idle => self.idle("Needs Spotify Premium."),
            WelcomeAction::Failed(error) => {
                self.idle(&error);
                self.sign_in_label.set_text("Try again");
            }
        }
    }
}

impl Welcome {
    fn idle(&self, note: &str) {
        self.sign_in.set_sensitive(true);
        self.sign_in_label.set_text("Sign in with Spotify");
        self.cancel.set_visible(false);
        self.note.set_text(note);
        self.turntable.stop();
    }
}

struct Turntable {
    platter: Platter,
    target: Rc<Cell<f64>>,
    motion: Rc<RefCell<Option<adw::TimedAnimation>>>,
}

impl Turntable {
    fn new(palette: Palette) -> Self {
        let platter = Platter::new(palette);
        let speed = Cell::new(0.0_f64);
        let target = Rc::new(Cell::new(rpm(IDLE_RPM)));
        let last = Cell::new(0_i64);
        platter.add_tick_callback({
            let target = target.clone();
            move |platter, clock| {
                let now = clock.frame_time();
                let dt = if last.get() == 0 {
                    0.0
                } else {
                    (now - last.get()) as f64 / 1_000_000.0
                };
                last.set(now);
                let wanted = target.get();
                let rate = if wanted > speed.get() {
                    SPIN_UP
                } else {
                    SPIN_DOWN
                };
                let next = speed.get() + (wanted - speed.get()) * (dt * rate).min(1.0);
                speed.set(if (next - wanted).abs() < 0.002 {
                    wanted
                } else {
                    next
                });
                platter.turn(speed.get() * dt);
                glib::ControlFlow::Continue
            }
        });

        Self {
            platter,
            target,
            motion: Rc::new(RefCell::new(None)),
        }
    }

    fn play(&self) {
        let swing = self.animate(Part::Angle, ARM_PLAY, 320, adw::Easing::EaseOutCubic);
        let target = self.target.clone();
        swing.connect_done(move |_| target.set(rpm(RPM)));
        let slide = self.animate(Part::Drop, 1.0, 260, adw::Easing::EaseOutCubic);
        let motion = self.motion.clone();
        slide.connect_done(move |_| {
            swing.play();
            *motion.borrow_mut() = Some(swing.clone());
        });
        slide.play();
        *self.motion.borrow_mut() = Some(slide);
    }

    fn stop(&self) {
        self.target.set(rpm(IDLE_RPM));
        let slide = self.animate(Part::Drop, 0.0, 220, adw::Easing::EaseInCubic);
        let lift = self.animate(Part::Angle, ARM_REST, 220, adw::Easing::EaseInCubic);
        let motion = self.motion.clone();
        lift.connect_done(move |_| {
            slide.play();
            *motion.borrow_mut() = Some(slide.clone());
        });
        lift.play();
        *self.motion.borrow_mut() = Some(lift);
    }

    fn animate(
        &self,
        part: Part,
        to: f64,
        duration: u32,
        easing: adw::Easing,
    ) -> adw::TimedAnimation {
        if let Some(previous) = self.motion.borrow_mut().take() {
            previous.skip();
        }
        let platter = self.platter.clone();
        let from = platter.part(part);
        let target = adw::CallbackAnimationTarget::new(move |value| platter.set_part(part, value));
        let animation = adw::TimedAnimation::new(&self.platter, from, to, duration, target);
        animation.set_easing(easing);

        animation
    }
}

#[derive(Clone, Copy)]
enum Part {
    Drop,
    Angle,
}

mod platter {
    use std::cell::{Cell, RefCell};

    use relm4::gtk::{self, gdk, glib, graphene, prelude::*, subclass::prelude::*};

    use crate::palette::Palette;

    pub struct Platter {
        pub angle: Cell<f64>,
        pub drop: Cell<f64>,
        pub arm: Cell<f64>,
        pub palette: RefCell<Palette>,
        disc: RefCell<Option<(i32, gdk::MemoryTexture)>>,
    }

    impl Default for Platter {
        fn default() -> Self {
            Self {
                angle: Cell::new(0.0),
                drop: Cell::new(0.0),
                arm: Cell::new(super::ARM_REST),
                palette: RefCell::default(),
                disc: RefCell::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Platter {
        const NAME: &'static str = "GatefoldPlatter";
        type Type = super::Platter;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Platter {}

    impl WidgetImpl for Platter {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let (width, height) = (widget.width() as f64, widget.height() as f64);
            if height == 0.0 {
                return;
            }
            let size = (height * 1.32).round() as i32;
            let mut disc = self.disc.borrow_mut();
            if disc.as_ref().map(|(cached, _)| *cached) != Some(size) {
                let texture = super::disc(size * widget.scale_factor(), &self.palette.borrow());
                *disc = Some((size, texture));
            }
            let Some((_, texture)) = disc.as_ref() else {
                return;
            };
            let half = size as f32 / 2.0;
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                (width * 0.26) as f32,
                (height * 0.70) as f32,
            ));
            snapshot.rotate(self.angle.get() as f32);
            snapshot.append_texture(
                texture,
                &graphene::Rect::new(-half, -half, size as f32, size as f32),
            );
            snapshot.restore();

            if self.drop.get() > 0.0 {
                let cr = snapshot.append_cairo(&graphene::Rect::new(
                    0.0,
                    0.0,
                    width as f32,
                    height as f32,
                ));
                super::draw_arm(
                    &cr,
                    (width * 0.26, height * 0.70),
                    size as f64 / 2.0,
                    self.drop.get(),
                    self.arm.get(),
                    &self.palette.borrow(),
                );
            }
        }
    }
}

glib::wrapper! {
    pub struct Platter(ObjectSubclass<platter::Platter>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Platter {
    fn new(palette: Palette) -> Self {
        let platter: Self = glib::Object::new();
        platter.imp().palette.replace(palette);
        platter.set_overflow(gtk::Overflow::Hidden);
        platter.set_hexpand(true);
        platter.set_vexpand(true);

        platter
    }

    fn turn(&self, degrees: f64) {
        let angle = self.imp().angle.get();
        self.imp().angle.set((angle + degrees) % 360.0);
        self.queue_draw();
    }

    fn part(&self, part: Part) -> f64 {
        match part {
            Part::Drop => self.imp().drop.get(),
            Part::Angle => self.imp().arm.get(),
        }
    }

    fn set_part(&self, part: Part, value: f64) {
        match part {
            Part::Drop => self.imp().drop.set(value),
            Part::Angle => self.imp().arm.set(value),
        }
        self.queue_draw();
    }
}

fn draw_arm(
    cr: &cairo::Context,
    (cx, cy): (f64, f64),
    radius: f64,
    drop: f64,
    angle: f64,
    palette: &Palette,
) {
    let rgb = |saturation: f64, value: f64| {
        let (r, g, b) = palette.tone(saturation, value);
        (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
    };
    let set = |cr: &cairo::Context, (r, g, b): (f64, f64, f64)| cr.set_source_rgb(r, g, b);

    let (px, py) = (
        cx + radius,
        cy - radius * 0.95 - radius * 1.45 * (1.0 - drop),
    );
    let reach = radius * 1.25;
    let theta = angle.to_radians();
    let (tx, ty) = (px + reach * theta.cos(), py + reach * theta.sin());
    let metal = rgb(0.06, 0.58);
    cr.set_line_cap(cairo::LineCap::Round);
    set(cr, metal);
    cr.set_line_width(radius * 0.02);
    cr.move_to(px, py);
    cr.line_to(tx, ty);
    let _ = cr.stroke();
    cr.set_line_width(radius * 0.045);
    cr.move_to(px, py);
    cr.line_to(
        px - radius * 0.16 * theta.cos(),
        py - radius * 0.16 * theta.sin(),
    );
    let _ = cr.stroke();
    set(cr, rgb(0.1, 0.3));
    cr.arc(px, py, radius * 0.06, 0.0, 2.0 * PI);
    let _ = cr.fill();
    set(cr, metal);
    cr.set_line_width(radius * 0.05);
    cr.move_to(tx, ty);
    cr.line_to(
        tx + radius * 0.11 * (theta + 0.9).cos(),
        ty + radius * 0.11 * (theta + 0.9).sin(),
    );
    let _ = cr.stroke();
    set(cr, rgb(0.55, 0.92));
    cr.arc(tx, ty, radius * 0.014, 0.0, 2.0 * PI);
    let _ = cr.fill();
}

fn rpm(turns: f64) -> f64 {
    turns / 60.0 * 360.0
}

fn disc(pixels: i32, palette: &Palette) -> gdk::MemoryTexture {
    let mut surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, pixels, pixels).expect("disc surface");
    {
        let cr = cairo::Context::new(&surface).expect("disc context");
        let radius = pixels as f64 / 2.0 - 1.0;
        draw(&cr, radius, palette);
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().expect("disc pixels");
    gdk::MemoryTexture::new(
        pixels,
        pixels,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &glib::Bytes::from(&data[..]),
        stride,
    )
}

fn draw(cr: &cairo::Context, radius: f64, palette: &Palette) {
    let (cx, cy) = (radius + 1.0, radius + 1.0);
    let rgb = |saturation: f64, value: f64| {
        let (r, g, b) = palette.tone(saturation, value);
        (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
    };
    let set = |cr: &cairo::Context, (r, g, b): (f64, f64, f64), alpha: f64| {
        cr.set_source_rgba(r, g, b, alpha)
    };

    set(cr, rgb(0.3, 0.06), 1.0);
    cr.arc(cx, cy, radius, 0.0, 2.0 * PI);
    let _ = cr.fill();

    let light = rgb(0.1, 0.6);
    let pitch = radius / 230.0;
    let mut r = radius * 0.4;
    let mut band = 0;
    while r < radius * 0.975 {
        let gap = band % 23 == 0;
        set(cr, light, if gap { 0.015 } else { 0.05 });
        cr.set_line_width(if gap { pitch * 0.85 } else { pitch * 0.38 });
        cr.arc(cx, cy, r, 0.0, 2.0 * PI);
        let _ = cr.stroke();
        r += if gap { pitch * 1.7 } else { pitch };
        band += 1;
    }

    cr.translate(cx, cy);
    for (spread, alpha) in [(0.34, 0.035), (0.13, 0.045)] {
        for base in [0.85, 0.85 + PI] {
            cr.arc(0.0, 0.0, radius * 0.985, base - spread, base + spread);
            cr.arc_negative(0.0, 0.0, radius * 0.39, base + spread, base - spread);
            cr.close_path();
            set(cr, light, alpha);
            let _ = cr.fill();
        }
    }
    let label = radius * 0.36;
    set(cr, rgb(0.55, 0.92), 1.0);
    cr.arc(0.0, 0.0, label, 0.0, 2.0 * PI);
    let _ = cr.fill();
    set(cr, rgb(0.6, 0.16), 0.35);
    cr.set_line_width(radius * 0.0025);
    cr.arc(0.0, 0.0, label * 0.86, 0.0, 2.0 * PI);
    let _ = cr.stroke();
    set(cr, rgb(0.6, 0.16), 0.55);
    cr.arc(label * 0.62, 0.0, label * 0.05, 0.0, 2.0 * PI);
    let _ = cr.fill();
    set(cr, rgb(0.14, 0.08), 1.0);
    cr.arc(0.0, 0.0, radius * 0.018, 0.0, 2.0 * PI);
    let _ = cr.fill();
}
