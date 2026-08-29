use gatefold_core::model::ArtistRef;
use relm4::gtk::{self, glib, prelude::*};

pub fn markup(artists: &[ArtistRef]) -> String {
    artists
        .iter()
        .map(|artist| {
            let name = glib::markup_escape_text(&artist.name);
            if artist.uri.is_empty() {
                name.to_string()
            } else {
                format!(
                    "<a href=\"{}\">{name}</a>",
                    glib::markup_escape_text(&artist.uri)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn label(artists: &[ArtistRef], open: impl Fn(ArtistRef) + 'static) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_markup(&markup(artists));
    label.add_css_class("track-artists");
    label.set_xalign(0.0);
    label.set_halign(gtk::Align::Start);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_focusable(false);
    let artists = artists.to_vec();
    label.connect_activate_link(move |_, uri| {
        if let Some(artist) = artists.iter().find(|artist| artist.uri == uri) {
            open(artist.clone());
        }
        glib::Propagation::Stop
    });

    label
}

pub fn names(artists: &[ArtistRef]) -> String {
    artists
        .iter()
        .map(|artist| artist.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
