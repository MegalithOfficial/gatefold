use relm4::gtk::{gdk, gdk::prelude::*};

#[derive(Default)]
pub struct Palette {
    pub hue: f64,
    pub saturation: f64,
}

impl Palette {
    pub fn from_cover(path: &std::path::Path) -> Self {
        let Ok(texture) = gdk::Texture::from_filename(path) else {
            return Self {
                hue: 0.0,
                saturation: 0.0,
            };
        };

        let (width, height) = (texture.width() as usize, texture.height() as usize);
        let stride = width * 4;
        let mut data = vec![0u8; stride * height];
        texture.download(&mut data, stride);
        let mut buckets = [(0.0f64, 0.0f64, 0usize); 24];

        for y in (0..height).step_by(3) {
            for x in (0..width).step_by(3) {
                let i = y * stride + x * 4;
                let (b, g, r) = (
                    data[i] as f64 / 255.0,
                    data[i + 1] as f64 / 255.0,
                    data[i + 2] as f64 / 255.0,
                );
                let (hue, saturation, value) = to_hsv(r, g, b);

                if saturation < 0.35 || !(0.2..0.95).contains(&value) {
                    continue;
                }

                let slot = ((hue / 360.0 * 24.0) as usize).min(23);
                buckets[slot].0 += hue;
                buckets[slot].1 += saturation;
                buckets[slot].2 += 1;
            }
        }

        let best = buckets
            .iter()
            .max_by_key(|bucket| bucket.2)
            .copied()
            .unwrap_or_default();

        if best.2 == 0 {
            return Self {
                hue: 0.0,
                saturation: 0.0,
            };
        }

        Self {
            hue: best.0 / best.2 as f64,
            saturation: (best.1 / best.2 as f64).min(0.85),
        }
    }

    pub fn tone(&self, saturation: f64, value: f64) -> (u8, u8, u8) {
        to_rgb(self.hue, self.saturation * saturation, value)
    }

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

pub fn to_hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };

    let hue = if hue < 0.0 { hue + 360.0 } else { hue };
    let saturation = if max == 0.0 { 0.0 } else { delta / max };

    (hue, saturation, max)
}

pub fn to_rgb(hue: f64, saturation: f64, value: f64) -> (u8, u8, u8) {
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
