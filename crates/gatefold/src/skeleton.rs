use relm4::gtk::{self, prelude::*};

pub fn bone(width: i32, height: i32) -> gtk::Box {
    let bone = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bone.add_css_class("skeleton");
    bone.set_size_request(width, height);
    bone.set_valign(gtk::Align::Center);
    bone.set_halign(gtk::Align::Start);

    bone
}

pub fn track_row(index: i32, plays: bool) -> gtk::Box {
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
    if plays {
        row.append(&bone(110, 10));
    }
    row.append(&bone(30, 10));

    row
}
