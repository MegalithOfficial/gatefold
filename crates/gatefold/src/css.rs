use crate::palette::Palette;

const GLOBAL: &str = include_str!("style.css");

const SHEETS: &[&str] = &[
    GLOBAL,
    crate::components::deck::CSS,
    crate::components::rack::CSS,
    crate::components::topbar::CSS,
    crate::pages::artist::CSS,
    crate::pages::discography::CSS,
    crate::pages::home::CSS,
    crate::pages::lyrics::CSS,
    crate::pages::playlist::CSS,
    crate::pages::queue::CSS,
    crate::pages::search::CSS,
    crate::pages::welcome::CSS,
];

pub fn stylesheet(palette: &Palette) -> String {
    let mut css = palette.css();
    for sheet in SHEETS {
        css.push_str(sheet);
    }

    css
}
