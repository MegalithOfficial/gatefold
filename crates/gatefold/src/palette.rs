#[derive(Default)]
pub struct Palette {
    pub hue: f64,
    pub saturation: f64,
}

impl Palette {
    pub fn css(&self) -> String {
        let tone = |saturation: f64, value: f64| {
            let (r, g, b) = to_rgb(self.hue, self.saturation * saturation, value);
            format!("#{:02X}{:02X}{:02X}", r, g, b)
        };

        [
            ("surface", tone(0.14, 0.08)),
            ("surface_low", tone(0.14, 0.11)),
            ("surface_mid", tone(0.14, 0.14)),
            ("surface_high", tone(0.14, 0.19)),
            ("surface_top", tone(0.14, 0.24)),
            ("sleeve", tone(0.45, 0.20)),
            ("primary", tone(0.55, 0.92)),
            ("on_primary", tone(0.60, 0.16)),
            ("primary_dim", tone(0.55, 0.36)),
            ("on_primary_dim", tone(0.20, 0.96)),
            ("on_surface", tone(0.04, 0.93)),
            ("on_surface_dim", tone(0.10, 0.70)),
            ("outline", tone(0.10, 0.44)),
        ]
        .iter()
        .map(|(name, hex)| format!("@define-color {name} {hex};\n"))
        .collect()
    }
}

fn to_rgb(hue: f64, saturation: f64, value: f64) -> (u8, u8, u8) {
    let c = value * saturation;
    let x = c * (1.0 - (((hue / 60.0) % 2.0) - 1.0).abs());
    let m = value - c;

    let (r, g, b) = match hue as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}
