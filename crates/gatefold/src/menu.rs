use std::rc::Rc;

use relm4::gtk::{self, gdk, glib, prelude::*};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackMenu {
    PlayNext,
    AddToQueue,
}

pub const TRACK: &[(&str, TrackMenu)] = &[
    ("Play next", TrackMenu::PlayNext),
    ("Add to queue", TrackMenu::AddToQueue),
];

pub fn attach<T: Copy + 'static>(
    row: &gtk::Button,
    items: &'static [(&'static str, T)],
    pick: impl Fn(T) + 'static,
) -> gtk::Button {
    let pick = Rc::new(pick);
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_SECONDARY);
    gesture.connect_pressed({
        let row = row.clone();
        let pick = pick.clone();
        move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            open(&row, items, pick.clone(), Some((x as i32, y as i32)));
        }
    });
    row.add_controller(gesture);

    let more = gtk::Button::from_icon_name("view-more-symbolic");
    more.add_css_class("track-more");
    more.set_valign(gtk::Align::Center);
    more.set_focus_on_click(false);
    more.set_tooltip_text(Some("More options"));
    more.connect_clicked(move |more| open(more, items, pick.clone(), None));

    more
}

fn open<T: Copy + 'static>(
    parent: &gtk::Button,
    items: &'static [(&'static str, T)],
    pick: Rc<impl Fn(T) + 'static>,
    at: Option<(i32, i32)>,
) {
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_halign(gtk::Align::Start);
    popover.remove_css_class("background");
    popover.add_css_class("quick-menu");
    if let Some((x, y)) = at {
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x, y, 1, 1)));
    }

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.set_width_request(180);
    for (name, item) in items {
        let label = gtk::Label::new(Some(name));
        label.set_xalign(0.0);
        let button = gtk::Button::builder().child(&label).build();
        button.add_css_class("quick-row");
        button.set_focus_on_click(false);
        button.connect_clicked({
            let popover = popover.clone();
            let pick = pick.clone();
            let item = *item;
            move |_| {
                popover.popdown();
                pick(item);
            }
        });
        column.append(&button);
    }
    popover.set_child(Some(&column));
    popover.connect_closed(|popover| {
        let popover = popover.clone();
        glib::idle_add_local_once(move || popover.unparent());
    });
    popover.set_parent(parent);
    popover.popup();
}

pub fn enqueue(playback: &gatefold_core::player::Playback, uri: &str, pick: TrackMenu) {
    match pick {
        TrackMenu::PlayNext => playback.play_next(vec![uri.to_owned()]),
        TrackMenu::AddToQueue => playback.add_to_queue(vec![uri.to_owned()]),
    }
}
