use gatefold_core::model::ArtistRef;
use relm4::gtk::{self, glib, prelude::*};

pub fn label(artists: &[ArtistRef], open: impl Fn(ArtistRef) + 'static) -> gtk::Label {
    let markup = artists
        .iter()
        .map(|artist| {
            format!(
                "<a href=\"{}\">{}</a>",
                glib::markup_escape_text(&artist.uri),
                glib::markup_escape_text(&artist.name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let label = gtk::Label::new(None);
    label.set_markup(&markup);
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
