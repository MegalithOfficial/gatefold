use relm4::{
    Sender,
    gtk::{self, glib, prelude::*},
};

use crate::app::AppAction;

pub fn install(window: &impl IsA<gtk::Widget>, sender: &Sender<AppAction>) {
    let table: &[(&str, fn() -> AppAction)] = &[
        ("<Control>k", || AppAction::FocusSearch),
        ("<Alt>Left", || AppAction::Back),
        ("<Alt>Right", || AppAction::Forward),
    ];

    let controller = gtk::ShortcutController::new();
    controller.set_scope(gtk::ShortcutScope::Global);
    for (accel, action) in table {
        let Some(trigger) = gtk::ShortcutTrigger::parse_string(accel) else {
            continue;
        };
        let sender = sender.clone();
        let callback = gtk::CallbackAction::new(move |_, _| {
            sender.emit(action());
            glib::Propagation::Stop
        });
        controller.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(callback)));
    }
    window.add_controller(controller);
}
